import SwiftUI

struct ManualDate: View {
    @Environment(AppModel.self) private var model
    let row: DateRow

    private var shown: Date {
        let candidates = [row.manual, row.current] + row.options.map(\.value)
        return candidates.lazy.compactMap { $0 }.first.map(WallClock.date) ?? Date()
    }

    var body: some View {
        HStack(spacing: 4) {
            DatePicker("Date", selection: Binding(
                get: { shown },
                set: { model.core.datesManual(uuid: row.uuid, at: WallClock.seconds($0)) }
            ), displayedComponents: [.date, .hourAndMinute])
            .labelsHidden()
            .environment(\.timeZone, WallClock.utc)
            if row.manualChosen {
                Button {
                    model.core.datesManual(uuid: row.uuid, at: nil)
                } label: {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(.tint)
                }
                .buttonStyle(.borderless)
                .help("Clear")
            }
        }
        .disabled(!model.dates.canPick)
    }
}

struct DatesPicker: View {
    @Environment(AppModel.self) private var model

    private func row(_ i: Int) -> DateRow? {
        _ = model.datesVersion
        return model.dateRows[i]
    }

    @ViewBuilder
    private func option(_ r: DateRow, _ key: DateKey) -> some View {
        if let o = r.options.first(where: { $0.key == key }) {
            Toggle(isOn: Binding(get: { o.chosen }, set: { _ in model.core.datesPick(uuid: r.uuid, key: o.key) })) {
                Text(o.value.map(WallClock.text) ?? "—").font(.callout.monospaced())
            }
            .toggleStyle(.button)
            .controlSize(.small)
            .disabled(o.value == nil || !model.dates.canPick)
            .opacity(o.sameDay ? 0.55 : 1)
            .help(o.sameDay ? "Same day as the current date" : "")
        }
    }

    var body: some View {
        let d = model.dates
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
                    if let r = row(ref.id) { Text(r.current.map(WallClock.text) ?? "no date").font(.callout.monospaced()).foregroundStyle(.secondary) }
                }
                .width(min: 90, ideal: 150)
                TableColumn(DateKey.create.label) { ref in if let r = row(ref.id) { option(r, .create) } }
                    .width(min: 90, ideal: 160)
                TableColumn(DateKey.original.label) { ref in if let r = row(ref.id) { option(r, .original) } }
                    .width(min: 90, ideal: 160)
                TableColumn(DateKey.modify.label) { ref in if let r = row(ref.id) { option(r, .modify) } }
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
                ForEach(DateKey.cases, id: \.self) { k in
                    Button("\(k.label) Date") { model.core.datesPickAll(key: k) }
                }
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
