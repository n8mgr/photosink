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
            ForEach(model.groups, id: \.name) { g in
                SwiftUI.Section(g.name) {
                    ForEach(g.tabs, id: \.name) { t in
                        Label(t.name, systemImage: t.tab.symbol)
                            .tag(t.tab)
                            .selectionDisabled(model.running && t.tab != model.tab)
                    }
                }
            }
        }
        .navigationSplitViewColumnWidth(min: 170, ideal: 200)
    }
}

struct TabInputs: View {
    @Environment(AppModel.self) private var model
    let info: TabInfo

    private func choose(_ spec: ArgSpec) {
        let panel = NSOpenPanel()
        let types = spec.extensions.compactMap { UTType(filenameExtension: $0) }
        panel.allowedContentTypes = types
        switch spec.picker {
        case .library:
            panel.canChooseFiles = true
            panel.canChooseDirectories = false
            panel.directoryURL = URL(fileURLWithPath: NSString(string: "~/Pictures").expandingTildeInPath)
        case .folderOrArchive:
            panel.canChooseFiles = true
            panel.canChooseDirectories = true
            panel.allowedContentTypes = types + [.folder]
        case .none:
            return
        }
        if panel.runModal() == .OK, let url = panel.url {
            var a = model.args
            a.arg = url.path
            model.setArgs(a)
        }
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            if let spec = info.arg {
                Text(spec.label).lineLimit(1)
            }
            VStack(alignment: .leading, spacing: 8) {
                if let spec = info.arg {
                    HStack {
                        TextField(spec.label, text: Binding(
                            get: { model.args.arg },
                            set: { v in var a = model.args; a.arg = v; model.setArgs(a) }
                        ), prompt: Text(spec.placeholder))
                        .labelsHidden()
                        .onSubmit { if spec.runOnEnter && model.blocker == nil { model.run() } }
                        if spec.picker != .none {
                            Button("Choose…") { choose(spec) }.fixedSize()
                        }
                    }
                }
                if let option = info.option {
                    Toggle(isOn: Binding(
                        get: { model.args.option },
                        set: { v in var a = model.args; a.option = v; model.setArgs(a) }
                    )) {
                        Text(option).lineLimit(1).truncationMode(.tail).help(option)
                    }
                }
                Text(info.hint)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
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
            ForEach(v.chips, id: \.label) { c in
                Toggle(isOn: Binding(get: { !c.hidden }, set: { _ in model.core.toggleChip(tab: tab, state: c.state, only: false) })) {
                    Text("\(c.label)  \(c.count)")
                }
            }
            Divider()
            Menu("Show Only") {
                ForEach(v.chips, id: \.label) { c in
                    Button(c.label) { model.core.toggleChip(tab: tab, state: c.state, only: true) }
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
        .help(filtering ? "\(v.hidden) rows hidden" : "Show or hide rows by result")
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
        if model.status.running == model.tab || model.status.running == nil {
            parts.append(model.status.text)
        }
        parts += v.stats.map { "\($0.label) \($0.value)" }
        if v.hidden > 0 { parts.append("\(v.hidden) hidden") }
        if let b = model.blocker, model.status.running == nil { parts = [b] }
        return parts.joined(separator: " · ")
    }

    var body: some View {
        @Bindable var model = model
        let datesShown = model.tab == .dates && model.dates.shown
        ZStack {
                ForEach(model.groups.flatMap(\.tabs), id: \.name) { t in
                    let shown = t.tab == model.tab && !(t.tab == .dates && datesShown)
                    StatusGrid(tab: t.tab)
                        .opacity(shown ? 1 : 0)
                        .allowsHitTesting(shown)
                }
                if datesShown {
                    DatesPicker()
                }
        }
        .frame(minWidth: 460, minHeight: 200)
        .safeAreaBar(edge: .top) {
            if let info = model.info {
                TabInputs(info: info)
            }
        }
        .navigationTitle(model.info?.name ?? "Photosink")
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
                    .disabled(model.status.running == model.tab || (model.tabs[model.tab]?.count ?? 0) == 0)
                }
            }
            ToolbarSpacer(.fixed)
            ToolbarItemGroup(placement: .primaryAction) {
                if model.info?.dryRun == true {
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
                    if model.status.running == model.tab {
                        ProgressView().controlSize(.small)
                    } else {
                        Label(model.runLabel, systemImage: "play.fill").labelStyle(.titleAndIcon)
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(model.blocker != nil)
                .help(model.blocker ?? model.runLabel)
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
    var body: some View {
        NavigationSplitView {
            Sidebar()
        } detail: {
            TabDetail()
        }
    }
}

final class Delegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main
struct PhotosinkApp: App {
    @NSApplicationDelegateAdaptor(Delegate.self) private var delegate
    @State private var model = AppModel()

    var body: some Scene {
        Window("Photosink", id: "main") {
            MainView().environment(model)
        }
        .defaultSize(width: 1000, height: 700)
        .windowResizability(.contentMinSize)
        SwiftUI.Settings {
            SettingsWindow().environment(model)
        }
    }
}
