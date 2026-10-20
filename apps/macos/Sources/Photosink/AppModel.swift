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
    @ObservationIgnored private var openingPhotos = false
    @ObservationIgnored let core: Core
    @ObservationIgnored private let relay: Relay
    @ObservationIgnored private var rows: [Tab: Pages<Row>] = [:]
    @ObservationIgnored private(set) var dateRows: Pages<DateRow>!
    @ObservationIgnored private var lastInGroup: [SidebarGroup: Tab] = [:]

    let available: [Tab]
    let fields: [Field]
    var settings: Settings?
    var settingsError: String?
    var status: Status
    var runningTab: Tab?
    var notice: String?
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
        available = core.tabs()
        fields = core.fields()
        status = core.status()
        runningTab = core.running()
        dates = core.datesView()
        let first = available.first ?? .consolidate
        tab = first
        args = core.runArgs(tab: first)
        relay.model = self
        dateRows = Pages { [core] start, count in core.datesRows(start: start, count: count) }
        for t in available {
            rows[t] = Pages { [core] start, count in core.gridRows(tab: t, start: start, count: count) }
            tabs[t] = TabState()
        }
        do {
            settings = try core.settings()
        } catch {
            settingsError = message(error, tab: first)
        }
    }

    var running: Bool { runningTab != nil }

    var blocker: Blocker? {
        _ = status
        _ = runningTab
        _ = settings
        _ = args
        return core.blocker(tab: tab)
    }

    var blockerText: String? {
        blocker?.text(for: tab)
    }

    var runLabel: String {
        tab.runLabel(args)
    }

    func tabs(in group: SidebarGroup) -> [Tab] {
        available.filter(group.contains)
    }

    func apply(_ change: Change) {
        switch change {
        case .status:
            status = core.status()
            runningTab = core.running()
            notice = nil
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
        if let g = SidebarGroup.allCases.first(where: { $0.contains(t) }) { lastInGroup[g] = t }
    }

    func selectGroup(_ group: SidebarGroup) {
        guard let t = lastInGroup[group] ?? tabs(in: group).first else { return }
        select(t)
    }

    func setArgs(_ a: RunArgs) {
        args = a
        core.setRunArgs(tab: tab, args: a)
    }

    func save(_ s: Settings) {
        settings = s
        do {
            try core.saveSettings(settings: s)
            settingsError = nil
        } catch {
            settingsError = message(error, tab: tab)
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
            notice = message(error, tab: tab)
        }
    }

    func applyDates() {
        do {
            try core.applyDates()
        } catch {
            notice = message(error, tab: .dates)
        }
    }

    func openInPhotos(_ id: String) {
        guard !openingPhotos else { return }
        openingPhotos = true
        Task {
            defer { openingPhotos = false }
            let error = await Task.detached { () -> String? in
                let escaped = id.replacingOccurrences(of: "\\", with: "\\\\")
                    .replacingOccurrences(of: "\"", with: "\\\"")
                let source = """
                    tell application id "com.apple.Photos"
                        activate
                        spotlight media item id "\(escaped)"
                    end tell
                    """
                guard let script = NSAppleScript(source: source) else {
                    return "Could not create the Open in Photos script."
                }
                var error: NSDictionary?
                script.executeAndReturnError(&error)
                return error.map { $0[NSAppleScript.errorMessage] as? String ?? $0.description }
            }.value
            if let error { notice = "Could not open in Photos: \(error)" }
        }
    }

    func open(_ path: String) {
        let url = URL(fileURLWithPath: path)
        guard FileManager.default.fileExists(atPath: path) else {
            notice = "\(path): \(String(cString: strerror(ENOENT)))"
            return
        }
        if !NSWorkspace.shared.open(url) {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        }
    }
}
