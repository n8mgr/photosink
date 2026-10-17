import AppKit
import SwiftUI

struct CommitField: View {
    let title: String
    let placeholder: String
    let value: String
    var monospaced = false
    let commit: (String) -> Void
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        TextField(title, text: $text, prompt: Text(placeholder))
            .font(monospaced ? .body.monospaced() : .body)
            .focused($focused)
            .onAppear { text = value }
            .onChange(of: value) { if !focused { text = value } }
            .onSubmit { commit(text) }
            .onChange(of: focused) { if !focused && text != value { commit(text) } }
    }
}

struct TemplateEditor: View {
    @Environment(AppModel.self) private var model
    let title: String
    let kind: TemplateKind
    let settings: Settings
    @State private var text = ""
    @State private var selection: TextSelection?
    @State private var preview: Preview?
    @FocusState private var focused: Bool

    private var stored: String { kind == .folder ? settings.path : settings.filename }

    private func with(_ t: String) -> Settings {
        var s = settings
        if kind == .folder { s.path = t } else { s.filename = t }
        return s
    }

    private func refresh() {
        preview = model.core.preview(settings: with(text), kind: kind)
    }

    private func commit() {
        refresh()
        if preview?.problem == nil && text != stored { model.save(with(text)) }
    }

    private func utf16(_ i: String.Index) -> UInt32 {
        UInt32(text.utf16.distance(from: text.startIndex, to: i))
    }

    private func insert(_ field: Field) {
        var start = UInt32(text.utf16.count), end = start
        if case let .selection(range)? = selection?.indices {
            start = utf16(range.lowerBound)
            end = utf16(range.upperBound)
        }
        let r = model.core.insertField(text: text, start: start, end: end, token: field.token, kind: kind)
        text = r.text
        let caret = String.Index(utf16Offset: Int(r.caret), in: text)
        selection = TextSelection(range: caret..<caret)
        focused = true
        commit()
    }

    var body: some View {
        LabeledContent {
            HStack {
                TextField(
                    title,
                    text: $text,
                    selection: $selection,
                    prompt: Text(kind == .folder ? model.options.folderPlaceholder : model.options.filenamePlaceholder)
                )
                .labelsHidden()
                .font(.body.monospaced())
                .focused($focused)
                .onSubmit(commit)
                .onChange(of: focused) { if !focused { commit() } }
                .onChange(of: text) { refresh() }
                Menu {
                    ForEach(model.fields, id: \.name) { g in
                        SwiftUI.Section(g.name) {
                            ForEach(g.fields, id: \.name) { f in
                                Toggle(isOn: Binding(get: { preview?.used.contains(f.name) == true }, set: { _ in insert(f) })) {
                                    Text(f.token)
                                    Text(f.help)
                                }
                            }
                        }
                    }
                } label: {
                    Label("Insert Field", systemImage: "plus")
                }
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Insert a field at the cursor")
            }
        } label: {
            Text(title)
            if let p = preview {
                if let problem = p.problem {
                    Text(problem).foregroundStyle(.red)
                } else {
                    Text("\(p.line) — \(p.sampleNote)")
                }
            }
        }
        .onAppear {
            text = stored
            refresh()
        }
        .onChange(of: stored) { if !focused { text = stored; refresh() } }
    }
}

/// An optional `YYYY-MM-DD` date: a checkbox to set it, then a date picker.
struct OptionalDate: View {
    let title: String
    let value: String
    let commit: (String) -> Void

    private static let format: DateFormatter = {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(secondsFromGMT: 0)
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()

    var body: some View {
        let date = Self.format.date(from: value)
        LabeledContent(title) {
            HStack {
                if let date {
                    DatePicker(title, selection: Binding(get: { date }, set: { commit(Self.format.string(from: $0)) }), displayedComponents: .date)
                        .labelsHidden()
                        .environment(\.timeZone, TimeZone(secondsFromGMT: 0)!)
                }
                Toggle(title, isOn: Binding(
                    get: { date != nil },
                    set: { on in commit(on ? Self.format.string(from: Date()) : "") }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
            }
        }
    }
}

struct SettingsWindow: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        SwiftUI.TabView {
            SwiftUI.Tab("Layout", systemImage: "folder") { page { layout($0, $1) } }
            SwiftUI.Tab("Dates", systemImage: "calendar") { page { dates($0, $1) } }
            SwiftUI.Tab("Filter", systemImage: "line.3.horizontal.decrease") { page { filter($0, $1) } }
            SwiftUI.Tab("Files", systemImage: "doc.on.doc") { page { files($0, $1) } }
        }
        .frame(width: 600)
        .scenePadding()
    }

    @ViewBuilder
    private func page<Content: View>(@ViewBuilder _ content: @escaping (Settings, Options) -> Content) -> some View {
        if let s = model.settings {
            Form {
                content(s, model.options)
                if let e = model.settingsError {
                    SwiftUI.Section {
                        Label(e, systemImage: "exclamationmark.triangle.fill").foregroundStyle(.red)
                    }
                }
            }
            .formStyle(.grouped)
            .frame(minHeight: 320)
        } else {
            ContentUnavailableView(
                "Settings Unavailable",
                systemImage: "exclamationmark.triangle",
                description: Text(model.settingsError ?? "")
            )
        }
    }

    private func set(_ s: Settings, _ change: (inout Settings) -> Void) {
        var s = s
        change(&s)
        model.save(s)
    }

    private func chooseFolder(_ s: Settings) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = URL(fileURLWithPath: s.exportDir)
        if panel.runModal() == .OK, let url = panel.url {
            set(s) { $0.exportDir = url.path }
        }
    }

    @ViewBuilder
    private func layout(_ s: Settings, _ o: Options) -> some View {
        SwiftUI.Section {
            LabeledContent("Export to") {
                HStack {
                    Text(s.exportDir.isEmpty ? "No folder chosen" : (s.exportDir as NSString).abbreviatingWithTildeInPath)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.head)
                        .help(s.exportDir)
                    Button("Choose…") { chooseFolder(s) }
                }
            }
        }
        SwiftUI.Section {
            TemplateEditor(title: "Folder", kind: .folder, settings: s)
            TemplateEditor(title: "Filename", kind: .filename, settings: s)
        } header: {
            Text("Layout")
        }
    }

    @ViewBuilder
    private func dates(_ s: Settings, _ o: Options) -> some View {
        SwiftUI.Section {
            Picker(selection: Binding(get: { s.dateSource }, set: { v in set(s) { $0.dateSource = v } })) {
                ForEach(o.dateSources, id: \.label) { Text($0.label).tag($0.value) }
            } label: {
                Text("Taken date")
                Text(o.dateSourceHint)
            }
            Picker(selection: Binding(get: { s.zone }, set: { v in set(s) { $0.zone = v } })) {
                ForEach(o.zones, id: \.label) { Text($0.label).tag($0.value) }
            } label: {
                Text("Folder clock")
                Text(o.zoneHint)
            }
            if s.zone == .fixed {
                CommitField(title: "Offset", placeholder: o.offsetPlaceholder, value: s.offset, monospaced: true) { v in
                    set(s) { $0.offset = v }
                }
            }
        }
    }

    @ViewBuilder
    private func filter(_ s: Settings, _ o: Options) -> some View {
        SwiftUI.Section {
            Picker("Media", selection: Binding(get: { s.media }, set: { v in set(s) { $0.media = v } })) {
                ForEach(o.media, id: \.label) { Text($0.label).tag($0.value) }
            }
            .pickerStyle(.segmented)
            Toggle(o.favoritesLabel, isOn: Binding(get: { s.favorites }, set: { v in set(s) { $0.favorites = v } }))
            OptionalDate(title: "Taken from", value: s.from) { v in set(s) { $0.from = v } }
            OptionalDate(title: "Taken to", value: s.to) { v in set(s) { $0.to = v } }
            CommitField(title: "Albums", placeholder: o.listPlaceholder, value: s.albums) { v in set(s) { $0.albums = v } }
            CommitField(title: "Keywords", placeholder: o.listPlaceholder, value: s.keywords) { v in set(s) { $0.keywords = v } }
        } footer: {
            Text(model.summaries?.filterHint ?? "").foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func files(_ s: Settings, _ o: Options) -> some View {
        SwiftUI.Section {
            Picker(selection: Binding(get: { s.edits }, set: { v in set(s) { $0.edits = v } })) {
                Text(o.editsOff).tag(false)
                Text(o.editsOn).tag(true)
            } label: {
                Text("Export")
                Text(o.editsHint)
            }
            Picker(selection: Binding(get: { s.metadata }, set: { v in set(s) { $0.metadata = v } })) {
                ForEach(o.metadata, id: \.label) { Text($0.label).tag($0.value) }
            } label: {
                Text("Metadata")
                Text(o.metadataHint)
            }
        }
    }
}
