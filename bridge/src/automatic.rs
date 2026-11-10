use crate::catalog::{self, RunArgs, Tab};
use crate::settings::Settings;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Schedule {
    due: HashMap<Tab, Instant>,
    interval: Duration,
}

impl Schedule {
    pub(crate) fn configure(
        &mut self,
        settings: &Settings,
        args: &HashMap<Tab, RunArgs>,
        now: Instant,
    ) {
        let interval = Duration::from_secs(u64::from(settings.check_interval_minutes) * 60);
        let first = if !self.interval.is_zero() && self.interval != interval {
            now + interval
        } else {
            now
        };
        if self.interval != interval {
            self.due.clear();
        }
        self.interval = interval;
        self.due
            .retain(|tab, _| args.get(tab).is_some_and(|a| a.automatic));
        for tab in catalog::tabs().into_iter().filter(|t| t.is_source()) {
            if args.get(&tab).is_some_and(|a| a.automatic) {
                self.due.entry(tab).or_insert(first);
            }
        }
    }

    pub(crate) fn ready(&self, now: Instant) -> Option<Tab> {
        catalog::tabs()
            .into_iter()
            .filter_map(|tab| self.due.get(&tab).map(|due| (tab, *due)))
            .filter(|(_, due)| *due <= now)
            .min_by_key(|(_, due)| *due)
            .map(|(tab, _)| tab)
    }

    pub(crate) fn finished(&mut self, tab: Tab, now: Instant) {
        if let Some(due) = self.due.get_mut(&tab) {
            *due = now + self.interval;
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        !self.due.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photosink::config::Config;

    #[test]
    fn requires_opt_in_and_never_schedules_maintenance() {
        let now = Instant::now();
        let settings = Settings::from_config(&Config::parse("export_dir = '/out'").unwrap());
        let mut args: HashMap<_, _> = catalog::tabs()
            .into_iter()
            .map(|tab| (tab, tab.default_args()))
            .collect();
        let mut schedule = Schedule::default();
        args.get_mut(&Tab::Clean).unwrap().automatic = true;
        schedule.configure(&settings, &args, now);
        assert_eq!(schedule.ready(now), None);
        args.get_mut(&Tab::OneDrive).unwrap().automatic = true;
        schedule.configure(&settings, &args, now);
        assert_eq!(schedule.ready(now), Some(Tab::OneDrive));
        schedule.finished(Tab::OneDrive, now);
        assert_eq!(schedule.ready(now), None);
        args.get_mut(&Tab::OneDrive).unwrap().automatic = false;
        schedule.configure(&settings, &args, now);
        assert!(!schedule.enabled());
    }

    #[test]
    fn waits_after_completion_and_does_not_catch_up_missed_intervals() {
        let now = Instant::now();
        let mut settings = Settings::from_config(&Config::parse("export_dir = '/out'").unwrap());
        settings.check_interval_minutes = 15;
        let args = [Tab::OneDrive, Tab::Dropbox]
            .into_iter()
            .map(|tab| {
                (
                    tab,
                    RunArgs {
                        automatic: true,
                        ..tab.default_args()
                    },
                )
            })
            .collect();
        let mut schedule = Schedule::default();
        schedule.configure(&settings, &args, now);
        let after_long_run = now + Duration::from_secs(3600);
        schedule.finished(Tab::OneDrive, after_long_run);
        assert_eq!(schedule.ready(after_long_run), Some(Tab::Dropbox));
        schedule.finished(Tab::Dropbox, after_long_run);
        assert_eq!(
            schedule.ready(after_long_run + Duration::from_secs(899)),
            None
        );
        assert_eq!(
            schedule.ready(after_long_run + Duration::from_secs(900)),
            Some(Tab::OneDrive)
        );
        settings.check_interval_minutes = 30;
        schedule.configure(&settings, &args, after_long_run);
        assert_eq!(
            schedule.ready(after_long_run + Duration::from_secs(1799)),
            None
        );
        assert_eq!(
            schedule.ready(after_long_run + Duration::from_secs(1800)),
            Some(Tab::OneDrive)
        );
    }
}
