import AppKit
import Observation

final class Relay: Listener, @unchecked Sendable {
    weak var model: AppModel?

    func changed(change: Change) {
        Task { @MainActor [weak model] in model?.apply(change) }
    }
}

/// Rows fetched from Core a page at a time, dropped whenever Core reports the tab changed.
final class Pages<T> {
    private var pages: [Int: [T]] = [:]
    private let size = 200
    private let fetch: (UInt32, UInt32) -> [T]

    init(fetch: @escaping (UInt32, UInt32) -> [T]) {
        self.fetch = fetch
    }

    func reset() {
        pages.removeAll(keepingCapacity: true)
    }

    subscript(index: Int) -> T? {
        let page = index / size
        if pages[page] == nil {
            pages[page] = fetch(UInt32(page * size), UInt32(size))
        }
        let rows = pages[page]!
        let offset = index % size
        return offset < rows.count ? rows[offset] : nil
    }
}

struct TabState: Equatable {
    var count = 0
    var version = 0
    var chips: [Chip] = []
    var hidden = 0
    var stats: [Stat] = []
}

@MainActor @Observable
final class AppModel {
    @ObservationIgnored let core: Core
    @ObservationIgnored private let relay: Relay
    @ObservationIgnored private var rows: [Tab: Pages<Row>] = [:]
    @ObservationIgnored private(set) var dateRows: Pages<DateRow>!
    @ObservationIgnored private var lastInGroup: [String: Tab] = [:]

    let groups: [Group]
    let options: Options
    let fields: [FieldGroup]
    var settings: Settings?
    var summaries: Summaries?
    var settingsError: String?
    var status: Status
    var tab: Tab
    var args: RunArgs
    var tabs: [Tab: TabState] = [:]
    var dates: DatesView
    var datesCount = 0
    var datesVersion = 0
    var follow = true
    var confirming: Confirmation?

    init() {
        let relay = Relay()
        let core = Core(listener: relay)
        self.relay = relay
        self.core = core
        groups = core.groups()
        options = core.options()
        fields = core.fields()
        status = core.status()
        dates = core.datesView()
        let first = groups.first?.tabs.first?.tab ?? .consolidate
        tab = first
        args = core.runArgs(tab: first)
        relay.model = self
        dateRows = Pages { [core] start, count in core.datesRows(start: start, count: count) }
        for g in groups {
            lastInGroup[g.name] = g.tabs.first?.tab
            for t in g.tabs {
                rows[t.tab] = Pages { [core] start, count in core.gridRows(tab: t.tab, start: start, count: count) }
                tabs[t.tab] = TabState()
            }
        }
        do {
            let s = try core.settings()
            settings = s
            summaries = core.summaries(settings: s)
        } catch {
            settingsError = message(error)
        }
    }

    var info: TabInfo? {
        groups.flatMap(\.tabs).first { $0.tab == tab }
    }

    var group: Group? {
        groups.first { $0.tabs.contains { $0.tab == tab } }
    }

    var running: Bool { status.running != nil }

    var blocker: String? {
        _ = status
        _ = settings
        _ = args
        return core.blocker(tab: tab)
    }

    var runLabel: String {
        _ = args
        return core.runLabel(tab: tab)
    }

    func message(_ error: Error) -> String {
        if case let CoreError.Failed(m) = error { return m }
        return error.localizedDescription
    }

    func apply(_ change: Change) {
        switch change {
        case .status:
            status = core.status()
        case let .grid(t):
            refresh(t)
        case .dates:
            dates = core.datesView()
            datesCount = Int(core.datesLen())
            dateRows.reset()
            datesVersion += 1
        }
    }

    func refresh(_ t: Tab) {
        rows[t]?.reset()
        var v = tabs[t] ?? TabState()
        v.count = Int(core.gridLen(tab: t))
        v.chips = core.chips(tab: t)
        v.hidden = Int(core.hiddenRows(tab: t))
        v.stats = core.stats(tab: t)
        v.version += 1
        tabs[t] = v
    }

    func row(_ t: Tab, _ index: Int) -> Row? {
        rows[t]?[index]
    }

    func select(_ t: Tab) {
        guard !running || t == tab else { return }
        tab = t
        args = core.runArgs(tab: t)
        if let g = group { lastInGroup[g.name] = t }
    }

    func selectGroup(_ name: String) {
        guard let t = lastInGroup[name] ?? groups.first(where: { $0.name == name })?.tabs.first?.tab else { return }
        select(t)
    }

    func setArgs(_ a: RunArgs) {
        args = a
        core.setRunArgs(tab: tab, args: a)
    }

    func save(_ s: Settings) {
        settings = s
        summaries = core.summaries(settings: s)
        do {
            try core.saveSettings(settings: s)
            settingsError = nil
        } catch {
            settingsError = message(error)
        }
    }

    func run() {
        if let c = core.confirmation(tab: tab) {
            confirming = c
        } else {
            start()
        }
    }

    func start() {
        confirming = nil
        do {
            try core.start(tab: tab)
        } catch {
            status = Status(text: message(error), error: true, running: nil)
        }
    }

    func applyDates() {
        do {
            try core.applyDates()
        } catch {
            status = Status(text: message(error), error: true, running: nil)
        }
    }

    func open(_ path: String) {
        do {
            try core.openPath(path: path)
        } catch {
            status = Status(text: message(error), error: true, running: status.running)
        }
    }
}
