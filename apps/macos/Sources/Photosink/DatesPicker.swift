import SwiftUI

struct ManualDate: View {
    @Environment(AppModel.self) private var model
    let row: DateRow
    @State private var text = ""
    @FocusState private var focused: Bool

    private func commit() {
        if text != row.manual { _ = model.core.datesManual(uuid: row.uuid, text: text) }
    }

    var body: some View {
        TextField(model.dates.manualPlaceholder, text: $text)
            .font(.callout.monospaced())
            .focused($focused)
            .onAppear { text = row.manual }
            .onChange(of: row.manual) { if !focused { text = row.manual } }
            .onSubmit(commit)
            .onChange(of: focused) { if !focused { commit() } }
            .overlay(alignment: .trailing) {
                if row.manualChosen {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(.tint).padding(.trailing, 4)
                } else if !row.manualError.isEmpty {
                    Image(systemName: "exclamationmark.circle.fill").foregroundStyle(.red).padding(.trailing, 4)
                }
            }
            .help(row.manualError)
            .disabled(!model.dates.canPick)
    }
}

struct DatesPicker: View {
    @Environment(AppModel.self) private var model

    private func row(_ i: Int) -> DateRow? {
        _ = model.datesVersion
        return model.dateRows[i]
    }

    private func option(_ r: DateRow, _ index: Int) -> some View {
        let o = r.options[index]
        return Toggle(isOn: Binding(get: { o.chosen }, set: { _ in model.core.datesPick(uuid: r.uuid, key: o.key) })) {
            Text(o.value.isEmpty ? "—" : o.value).font(.callout.monospaced())
        }
        .toggleStyle(.button)
        .controlSize(.small)
        .disabled(o.value.isEmpty || !model.dates.canPick)
        .opacity(o.sameDay ? 0.55 : 1)
        .help(o.sameDay ? "Same day as the current date" : "")
    }

    var body: some View {
        let d = model.dates
        let labels = model.dateRows[0]?.options.map(\.label) ?? ["Create", "Original", "Modify"]
        VStack(spacing: 0) {
            Table(Refs(count: model.datesCount)) {
                TableColumn("Asset") { ref in
                    if let r = row(ref.id) {
                        HStack(spacing: 0) {
                            Text(r.folder + "/").foregroundStyle(.secondary)
                            Text(r.stem)
                            if r.video { Image(systemName: "video").foregroundStyle(.secondary).padding(.leading, 4) }
                        }
                        .lineLimit(1)
                        .truncationMode(.head)
                        .help(r.uuid)
                    }
                }
                .width(min: 120, ideal: 220)
                TableColumn("Current") { ref in
                    if let r = row(ref.id) { Text(r.current).font(.callout.monospaced()).foregroundStyle(.secondary) }
                }
                .width(min: 90, ideal: 150)
                TableColumn(labels[0]) { ref in if let r = row(ref.id) { option(r, 0) } }
                    .width(min: 90, ideal: 160)
                TableColumn(labels[1]) { ref in if let r = row(ref.id) { option(r, 1) } }
                    .width(min: 90, ideal: 160)
                TableColumn(labels[2]) { ref in if let r = row(ref.id) { option(r, 2) } }
                    .width(min: 90, ideal: 160)
                TableColumn("Other date") { ref in
                    if let r = row(ref.id) { ManualDate(row: r).id(r.uuid) }
                }
                .width(min: 140, ideal: 220)
            }
            .tableStyle(.inset(alternatesRowBackgrounds: true))
            .overlay {
                if model.datesCount == 0 {
                    if d.scanning {
                        ProgressView(d.emptyText)
                    } else {
                        ContentUnavailableView(d.emptyText, systemImage: "calendar")
                    }
                }
            }
        }
    }
}

struct DatesToolbar: ToolbarContent {
    @Environment(AppModel.self) private var model

    var body: some ToolbarContent {
        let d = model.dates
        ToolbarItemGroup {
            if d.scanning {
                ProgressView(value: d.progress)
                    .progressViewStyle(.circular)
                    .controlSize(.small)
            }
            Menu {
                Button("Create Date") { model.core.datesPickAll(key: .create) }
                Button("Original Date") { model.core.datesPickAll(key: .original) }
                Button("Modify Date") { model.core.datesPickAll(key: .modify) }
                Divider()
                Button("None") { model.core.datesClear() }
            } label: {
                Label("Choose All", systemImage: "checklist")
            }
            .disabled(!d.canPick)
        }
        ToolbarSpacer(.fixed)
        ToolbarItem {
            Button(d.applyLabel) { model.applyDates() }
                .buttonStyle(.borderedProminent)
                .disabled(!d.canApply)
        }
    }
}
