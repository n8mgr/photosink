import AppKit
import SwiftUI
import UniformTypeIdentifiers

extension Tab {
    var symbol: String {
        switch self {
        case .applePhotos: "photo.on.rectangle.angled"
        case .oneDrive: "cloud"
        case .dropbox: "shippingbox"
        case .googleTakeout: "archivebox"
        case .consolidate: "folder.badge.gearshape"
        case .clean: "trash"
        case .dates: "calendar.badge.clock"
        }
    }
}

struct Sidebar: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        List(selection: Binding(get: { model.tab }, set: { if let t = $0 { model.select(t) } })) {
            ForEach(SidebarGroup.allCases, id: \.self) { g in
                let tabs = model.tabs(in: g)
                if !tabs.isEmpty {
                    SwiftUI.Section(g.name) {
                        ForEach(tabs, id: \.self) { t in
                            Label(t.name, systemImage: t.symbol)
                                .tag(t)
                                .selectionDisabled(model.running && t != model.runningTab)
                        }
                    }
                }
            }
        }
        .navigationSplitViewColumnWidth(min: 170, ideal: 200)
    }
}

struct TabInputs: View {
    @Environment(AppModel.self) private var model

    private var arg: Binding<String> {
        Binding(get: { model.args.arg }, set: { v in var a = model.args; a.arg = v; model.setArgs(a) })
    }

    private var automatic: some View {
        Toggle("Export automatically", isOn: Binding(
            get: { model.args.automatic },
            set: { v in var a = model.args; a.automatic = v; model.setArgs(a) }
        ))
        .help("Include this source when automatic exports are enabled in Settings")
    }

    private func setArg(_ url: URL) {
        var a = model.args
        a.arg = url.path
        model.setArgs(a)
    }

    private func chooseLibrary() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "photoslibrary")].compactMap { $0 }
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.directoryURL = URL(fileURLWithPath: NSString(string: "~/Pictures").expandingTildeInPath)
        if panel.runModal() == .OK, let url = panel.url { setArg(url) }
    }

    private func chooseTakeout() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = ["zip", "tgz", "gz"].compactMap { UTType(filenameExtension: $0) } + [.folder]
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        if panel.runModal() == .OK, let url = panel.url { setArg(url) }
    }

    private func field(_ label: String, _ placeholder: String, runOnEnter: Bool = false, choose: (() -> Void)? = nil) -> some View {
        HStack {
            TextField(label, text: arg, prompt: Text(placeholder))
                .labelsHidden()
                .onSubmit { if runOnEnter && model.blocker == nil { model.run() } }
            if let choose {
                Button("Choose…", action: choose).fixedSize()
            }
        }
    }

    private func inputs(_ label: String?, hint: String, @ViewBuilder _ controls: () -> some View) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            if let label {
                Text(label).lineLimit(1)
            }
            VStack(alignment: .leading, spacing: 8) {
                controls()
                Text(hint)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    var body: some View {
        SwiftUI.Group {
            switch model.tab {
            case .applePhotos:
                inputs("Library", hint: Tab.applePhotos.hint) {
                    field("Library", "Photos library", choose: chooseLibrary)
                    automatic
                }
            case .oneDrive:
                inputs("Folder", hint: Tab.oneDrive.hint) {
                    field("Folder", "Folder under the drive root; empty for the whole drive")
                    automatic
                }
            case .dropbox:
                inputs("Folder", hint: Tab.dropbox.hint) {
                    field("Folder", "Folder in your Dropbox; empty for everything")
                    automatic
                }
            case .googleTakeout:
                inputs("Takeout", hint: Tab.googleTakeout.hint) {
                    field("Takeout", "Folder of archives, a single archive, or an extracted tree", choose: chooseTakeout)
                    automatic
                }
            case .consolidate:
                inputs(nil, hint: Tab.consolidate.hint) {}
            case .clean:
                inputs(nil, hint: Tab.clean.hint) {
                    let option = "Also delete files the state doesn't reference"
                    Toggle(isOn: Binding(
                        get: { model.args.option },
                        set: { v in var a = model.args; a.option = v; model.setArgs(a) }
                    )) {
                        Text(option).lineLimit(1).truncationMode(.tail).help(option)
                    }
                }
            case .dates:
                inputs("Find", hint: Tab.dates.hint) {
                    field("Find", "Leave empty to list only assets whose date disagrees with the file", runOnEnter: true)
                }
            }
        }
        .disabled(model.running)
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
    }
}

struct FilterMenu: View {
    @Environment(AppModel.self) private var model
    let tab: Tab

    var body: some View {
        let v = model.tabs[tab] ?? TabState()
        let filtering = v.chips.contains(where: \.hidden)
        Menu {
            ForEach(v.chips, id: \.state) { c in
                Toggle(isOn: Binding(get: { !c.hidden }, set: { _ in model.core.toggleChip(tab: tab, state: c.state, only: false) })) {
                    Text("\(c.state.label)  \(c.count.formatted())")
                }
            }
            Divider()
            Menu("Show Only") {
                ForEach(v.chips, id: \.state) { c in
                    Button(c.state.label) { model.core.toggleChip(tab: tab, state: c.state, only: true) }
                }
            }
            Button("Show All") { model.core.showAll(tab: tab) }
                .disabled(!filtering)
        } label: {
            Label("Filter", systemImage: filtering
                ? "line.3.horizontal.decrease.circle.fill"
                : "line.3.horizontal.decrease.circle")
        }
        .disabled(v.chips.isEmpty)
        .help(filtering ? "\(v.hidden.formatted()) rows hidden" : "Show or hide rows by result")
    }
}

struct TabDetail: View {
    @Environment(AppModel.self) private var model

    private var subtitle: String {
        if model.tab == .dates && model.dates.shown {
            return model.dates.summary
        }
        let v = model.tabs[model.tab] ?? TabState()
        var parts: [String] = []
        if let n = model.notice {
            parts.append(n)
        } else if model.runningTab == model.tab || model.runningTab == nil {
            parts.append(model.status.text)
        }
        parts += v.stats.map { "\($0.kind.label) \($0.count.formatted())" }
        if v.hidden > 0 { parts.append("\(v.hidden.formatted()) hidden") }
        if let b = model.blockerText, model.runningTab == nil { parts = [b] }
        return parts.joined(separator: " · ")
    }

    var body: some View {
        @Bindable var model = model
        let datesShown = model.tab == .dates && model.dates.shown
        ZStack {
                ForEach(model.available, id: \.self) { t in
                    let shown = t == model.tab && !(t == .dates && datesShown)
                    StatusGrid(tab: t)
                        .opacity(shown ? 1 : 0)
                        .allowsHitTesting(shown)
                }
                if datesShown {
                    DatesPicker()
                }
        }
        .frame(minWidth: 460, minHeight: 200)
        .safeAreaBar(edge: .top) {
            TabInputs()
        }
        .navigationTitle(model.tab.name)
        .navigationSubtitle(subtitle)
        .toolbar {
            if datesShown {
                DatesToolbar()
            } else {
                ToolbarItemGroup {
                    FilterMenu(tab: model.tab)
                    Toggle(isOn: $model.follow) {
                        Label("Follow", systemImage: "arrow.down.to.line")
                    }
                    .help("Scroll to new rows as they arrive")
                    Button {
                        model.core.clear(tab: model.tab)
                    } label: {
                        Label("Clear", systemImage: "xmark.circle")
                    }
                    .help("Clear this tab's rows")
                    .disabled(model.runningTab == model.tab || (model.tabs[model.tab]?.count ?? 0) == 0)
                }
            }
            ToolbarSpacer(.fixed)
            ToolbarItemGroup(placement: .primaryAction) {
                if model.tab != .dates {
                    Toggle(isOn: Binding(
                        get: { model.args.dry },
                        set: { v in var a = model.args; a.dry = v; model.setArgs(a) }
                    )) {
                        Label("Dry Run", systemImage: "eye")
                    }
                    .help("List what would happen without changing anything")
                    .disabled(model.running)
                }
                Button {
                    model.run()
                } label: {
                    if model.runningTab == model.tab {
                        ProgressView().controlSize(.small)
                    } else {
                        Label(model.runLabel, systemImage: "play.fill").labelStyle(.titleAndIcon)
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(model.blocker != nil)
                .help(model.blockerText ?? model.runLabel)
            }
        }
        .confirmationDialog(
            model.confirming?.title ?? "",
            isPresented: Binding(get: { model.confirming != nil }, set: { if !$0 { model.confirming = nil } }),
            presenting: model.confirming
        ) { c in
            Button(c.ok, role: .destructive) { model.start() }
            Button("Cancel", role: .cancel) { model.confirming = nil }
        } message: { c in
            Text(c.message)
        }
    }
}

struct MainView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow
    let delegate: Delegate

    var body: some View {
        NavigationSplitView {
            Sidebar()
        } detail: {
            TabDetail()
        }
        .onAppear {
            delegate.model = model
            delegate.openMainWindow = { [openWindow] in openWindow(id: "main") }
        }
    }
}

@MainActor
final class Delegate: NSObject, NSApplicationDelegate {
    weak var model: AppModel?
    var openMainWindow: (() -> Void)?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        model?.settings?.keepRunningAfterClose != true
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        if !hasVisibleWindows {
            openMainWindow?()
            return false
        }
        return true
    }
}

struct BackgroundMenu: View {
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        Button("Open Photosink") {
            openWindow(id: "main")
            NSApp.activate()
        }
        Button("Settings…") {
            openSettings()
            NSApp.activate()
        }
        Divider()
        Button("Quit Photosink") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }
}

@main
struct PhotosinkApp: App {
    @NSApplicationDelegateAdaptor(Delegate.self) private var delegate
    @State private var model = AppModel()

    var body: some Scene {
        Window("Photosink", id: "main") {
            MainView(delegate: delegate).environment(model)
        }
        .defaultSize(width: 1000, height: 700)
        .windowResizability(.contentMinSize)
        SwiftUI.Settings {
            SettingsWindow().environment(model)
        }
        MenuBarExtra("Photosink", systemImage: "photo.on.rectangle.angled") {
            BackgroundMenu()
        }
    }
}
