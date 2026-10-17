import AppKit
import SwiftUI

struct Ref: Identifiable, Hashable {
    let id: Int
}

/// Row indices only; cells fetch their content from Core as they scroll into view.
struct Refs: RandomAccessCollection {
    let count: Int
    var startIndex: Int { 0 }
    var endIndex: Int { count }
    subscript(i: Int) -> Ref { Ref(id: i) }
}

struct DetailText: View {
    let detail: Detail

    var body: some View {
        switch detail.kind {
        case .new:
            Text("\(Text("→").foregroundStyle(.tertiary)) \(detail.text)")
        case .duplicate:
            Text("\(Text("=").foregroundStyle(.tertiary)) \(Text(detail.text).foregroundStyle(.secondary))")
        case .metadata:
            Text("\(Text("xmp").font(.caption2).foregroundStyle(.tertiary)) \(Text(detail.text).foregroundStyle(.secondary))")
        case .note:
            Text(detail.text).foregroundStyle(.secondary)
        case .error:
            Text(detail.text).foregroundStyle(.red)
        }
    }
}

struct StatusGrid: View {
    @Environment(AppModel.self) private var model
    let tab: Tab

    private func row(_ i: Int) -> Row? {
        _ = model.tabs[tab]?.version
        return model.row(tab, i)
    }

    private func copyText(_ r: Row) -> String {
        ([r.position + r.of, r.tag, r.title] + r.details.map(\.text) + r.facts.map { "\($0.value) \($0.label)" })
            .filter { !$0.isEmpty }
            .joined(separator: "\t")
    }

    var body: some View {
        let v = model.tabs[tab] ?? TabState()
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                Table(Refs(count: v.count)) {
                    TableColumn("#") { ref in
                        if let r = row(ref.id) {
                            position(r)
                        }
                    }
                    .width(min: 36, ideal: 64, max: 120)
                    TableColumn("Item") { ref in
                        if let r = row(ref.id) {
                            item(r)
                        }
                    }
                    .width(min: 120, ideal: 260)
                    TableColumn("Result") { ref in
                        if let r = row(ref.id) {
                            result(r)
                        }
                    }
                }
                .tableStyle(.inset(alternatesRowBackgrounds: true))
                .contextMenu(forSelectionType: Ref.ID.self) { ids in
                    Button("Copy") {
                        let text = ids.sorted().compactMap { row($0) }.map(copyText).joined(separator: "\n")
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(text, forType: .string)
                    }
                    .disabled(ids.isEmpty)
                }
                .overlay {
                    if v.count == 0 {
                        ContentUnavailableView(
                            v.hidden > 0 ? "Every Row Is Filtered Out" : "No Results",
                            systemImage: v.hidden > 0 ? "line.3.horizontal.decrease.circle" : "tray",
                            description: Text(v.hidden > 0 ? "Use Filter to show them again." : "Run to see each asset here.")
                        )
                    }
                }
                .onChange(of: v.count) {
                    if model.follow && v.count > 0 {
                        proxy.scrollTo(Ref(id: v.count - 1).id, anchor: .bottom)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func position(_ r: Row) -> some View {
        switch r.kind {
        case .asset:
            Text("\(r.position)\(Text(r.of).foregroundStyle(.tertiary))")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
        case .clean:
            Text(r.tag.uppercased())
                .font(.caption2)
                .foregroundStyle(r.failed ? .red : .secondary)
        case .run:
            Image(systemName: "play.fill").foregroundStyle(.tertiary).font(.caption2)
        case .error:
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red).font(.caption)
        case .signIn:
            Image(systemName: "key.fill").foregroundStyle(.secondary).font(.caption)
        }
    }

    @ViewBuilder
    private func item(_ r: Row) -> some View {
        switch r.kind {
        case .run:
            Text(r.title).fontWeight(.semibold).lineLimit(1)
        case .signIn:
            HStack {
                Text(r.tag).foregroundStyle(.secondary)
                if !r.title.isEmpty {
                    Text(r.title).font(.title3.monospaced().weight(.semibold)).textSelection(.enabled)
                }
            }
        case .error:
            Text(r.title).foregroundStyle(.red).textSelection(.enabled)
        case .clean:
            Text(r.title).font(.callout.monospaced()).lineLimit(1).truncationMode(.middle).help(r.title)
        case .asset:
            Text(r.title).foregroundStyle(r.failed ? .red : .primary).lineLimit(1).truncationMode(.middle)
        }
    }

    @ViewBuilder
    private func result(_ r: Row) -> some View {
        switch r.kind {
        case .run:
            HStack(spacing: 14) {
                ForEach(Array(r.facts.enumerated()), id: \.offset) { _, f in
                    Text("\(Text(f.value).monospacedDigit())\(Text(f.value.isEmpty ? f.label : " \(f.label)").foregroundStyle(.secondary))")
                        .lineLimit(1)
                        .help(f.tooltip)
                }
                Spacer(minLength: 0)
                if !r.logPath.isEmpty {
                    Button("Log File") { model.open(r.logPath) }
                        .buttonStyle(.link)
                        .help(r.logPath)
                }
            }
        case .signIn:
            Link(r.url, destination: URL(string: r.url) ?? URL(string: "https://microsoft.com/devicelogin")!)
        case .error:
            EmptyView()
        case .asset, .clean:
            HStack(spacing: 12) {
                ForEach(Array(r.details.enumerated()), id: \.offset) { _, d in
                    DetailText(detail: d)
                        .font(.callout.monospaced())
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(d.text)
                }
            }
        }
    }
}
