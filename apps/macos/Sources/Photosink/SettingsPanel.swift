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

    private var check: TemplateCheck? {
        guard let p = preview else { return nil }
        return kind == .folder ? p.folder : p.filename
    }

    private func refresh() {
        preview = model.core.preview(settings: with(text))
    }

    private func commit() {
        refresh()
        if check?.unknown.isEmpty != false && text != stored { model.save(with(text)) }
    }

    private func part(_ s: String, _ edited: Bool) -> Text {
        edited ? Text(s).bold() : Text(s)
    }

    private func insert(_ field: Field) {
        var range = text.endIndex..<text.endIndex
        if case let .selection(r)? = selection?.indices {
            range = r
        }
        var token = field.token
        if let before = text[..<range.lowerBound].last, !"/_-{|.".contains(before) {
            token.insert(kind.separator, at: token.startIndex)
        }
        let offset = text.distance(from: text.startIndex, to: range.lowerBound) + token.count
        text.replaceSubrange(range, with: token)
        let caret = text.index(text.startIndex, offsetBy: offset, limitedBy: text.endIndex) ?? text.endIndex
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
                    prompt: Text(kind.placeholder)
                )
                .labelsHidden()
                .font(.body.monospaced())
                .focused($focused)
                .onSubmit(commit)
                .onChange(of: focused) { if !focused { commit() } }
                .onChange(of: text) { refresh() }
                Menu {
                    ForEach(FieldGroup.cases, id: \.self) { g in
                        SwiftUI.Section(g.name) {
                            ForEach(model.fields.filter { $0.group == g }, id: \.name) { f in
                                Toggle(isOn: Binding(get: { check?.used.contains(f.name) == true }, set: { _ in insert(f) })) {
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
            if let p = preview, let c = check {
                if let problem = c.problem {
                    Text(problem).foregroundStyle(.red)
                } else {
                    Text("\(p.rootText)/\(part(p.dirText, kind == .folder))/\(part(p.file, kind == .filename)).\(p.ext) — \(p.sampleNote)")
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
            SwiftUI.Tab("Layout", systemImage: "folder") { page { layout($0) } }
            SwiftUI.Tab("Dates", systemImage: "calendar") { page { dates($0) } }
            SwiftUI.Tab("Filter", systemImage: "line.3.horizontal.decrease") { page { filter($0) } }
            SwiftUI.Tab("Files", systemImage: "doc.on.doc") { page { files($0) } }
        }
        .frame(width: 600)
        .scenePadding()
    }

    @ViewBuilder
    private func page<Content: View>(@ViewBuilder _ content: @escaping (Settings) -> Content) -> some View {
        if let s = model.settings {
            Form {
                content(s)
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
    private func layout(_ s: Settings) -> some View {
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
    private func dates(_ s: Settings) -> some View {
        SwiftUI.Section {
            Picker(selection: Binding(get: { s.dateSource }, set: { v in set(s) { $0.dateSource = v } })) {
                ForEach(DateSource.cases, id: \.self) { Text($0.label).tag($0) }
            } label: {
                Text("Taken date")
                Text(DateSource.hint)
            }
            Picker(selection: Binding(get: { s.zone }, set: { v in set(s) { $0.zone = v } })) {
                ForEach(ZoneMode.cases, id: \.self) { Text($0.label).tag($0) }
            } label: {
                Text("Folder clock")
                Text(ZoneMode.hint)
            }
            if s.zone == .fixed {
                CommitField(title: "Offset", placeholder: ZoneMode.offsetPlaceholder, value: s.offset, monospaced: true) { v in
                    set(s) { $0.offset = v }
                }
            }
        }
    }

    @ViewBuilder
    private func filter(_ s: Settings) -> some View {
        SwiftUI.Section {
            Picker("Media", selection: Binding(get: { s.media }, set: { v in set(s) { $0.media = v } })) {
                ForEach(Media.cases, id: \.self) { Text($0.label).tag($0) }
            }
            .pickerStyle(.segmented)
            Toggle("Favourites only", isOn: Binding(get: { s.favorites }, set: { v in set(s) { $0.favorites = v } }))
            OptionalDate(title: "Taken from", value: s.from) { v in set(s) { $0.from = v } }
            OptionalDate(title: "Taken to", value: s.to) { v in set(s) { $0.to = v } }
            CommitField(title: "Albums", placeholder: "Comma separated; any match", value: s.albums) { v in set(s) { $0.albums = v } }
            CommitField(title: "Keywords", placeholder: "Comma separated; any match", value: s.keywords) { v in set(s) { $0.keywords = v } }
        } footer: {
            Text(s.filterHint).foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func files(_ s: Settings) -> some View {
        SwiftUI.Section {
            Picker(selection: Binding(get: { s.edits }, set: { v in set(s) { $0.edits = v } })) {
                Text("Originals").tag(false)
                Text("Originals + edits").tag(true)
            } label: {
                Text("Export")
                Text("edits are the rendered version plus the .AAE adjustment data")
            }
            Picker(selection: Binding(get: { s.metadata }, set: { v in set(s) { $0.metadata = v } })) {
                ForEach(Metadata.cases, id: \.self) { Text($0.label).tag($0) }
            } label: {
                Text("Metadata")
                Text(Metadata.hint)
            }
        }
    }
}
