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

extension Detail {
    var text: String {
        switch self {
        case let .placed(path), let .metadata(path): path
        case let .duplicate(existing): existing
        case let .redated(from, to): "\(from.map(WallClock.text) ?? "none") → \(WallClock.text(to))"
        case let .error(subject, message): subject.map { "\($0): \(message)" } ?? message
        case .nothingNew: "nothing new"
        }
    }
}

extension Row {
    var copyText: String {
        let parts: [String]
        switch self {
        case let .run(title, facts, _):
            parts = [title?.text ?? ""] + facts.map { [$0.value, $0.label].filter { !$0.isEmpty }.joined(separator: " ") }
        case let .asset(position, name, _, details, _):
            parts = [position.map { "\($0.n.formatted())/\($0.total.formatted())" } ?? "", name ?? "…"] + details.map(\.text)
        case let .clean(path, reason, outcome, _):
            parts = [reason.label, path, outcome.label]
        case let .error(subject, message):
            parts = [subject.map { "\($0): \(message)" } ?? message]
        case let .signIn(prompt, url):
            if case let .deviceCode(code) = prompt {
                parts = [prompt.tag, code, url]
            } else {
                parts = [prompt.tag, url]
            }
        }
        return parts.filter { !$0.isEmpty }.joined(separator: "\t")
    }
}

struct DetailText: View {
    let detail: Detail

    var body: some View {
        switch detail {
        case let .placed(path):
            Text("\(Text("→").foregroundStyle(.tertiary)) \(path)")
        case let .duplicate(existing):
            Text("\(Text("=").foregroundStyle(.tertiary)) \(Text(existing).foregroundStyle(.secondary))")
        case let .metadata(path):
            Text("\(Text("xmp").font(.caption2).foregroundStyle(.tertiary)) \(Text(path).foregroundStyle(.secondary))")
        case let .redated(from, to):
            Text("\(Text(from.map(WallClock.text) ?? "none").foregroundStyle(.secondary))  \(Text("→").foregroundStyle(.tertiary)) \(WallClock.text(to))")
        case .error:
            Text(detail.text).foregroundStyle(.red)
        case .nothingNew:
            Text(detail.text).foregroundStyle(.secondary)
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
                        let text = ids.sorted().compactMap { row($0) }.map(\.copyText).joined(separator: "\n")
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
        switch r {
        case let .asset(position, _, _, _, _):
            if let position {
                Text("\(position.n.formatted())\(Text("/\(position.total.formatted())").foregroundStyle(.tertiary))")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        case let .clean(_, reason, _, failed):
            Text(reason.label.uppercased())
                .font(.caption2)
                .foregroundStyle(failed ? .red : .secondary)
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
        switch r {
        case let .run(title, _, _):
            Text(title?.text ?? "").fontWeight(.semibold).lineLimit(1)
        case let .signIn(prompt, _):
            HStack {
                Text(prompt.tag).foregroundStyle(.secondary)
                if case let .deviceCode(code) = prompt {
                    Text(code).font(.title3.monospaced().weight(.semibold)).textSelection(.enabled)
                }
            }
        case let .error(subject, message):
            Text(subject.map { "\($0): \(message)" } ?? message).foregroundStyle(.red).textSelection(.enabled)
        case let .clean(path, _, _, _):
            Text(path).font(.callout.monospaced()).lineLimit(1).truncationMode(.middle).help(path)
        case let .asset(_, name, _, _, failed):
            Text(name ?? "…").foregroundStyle(failed ? .red : .primary).lineLimit(1).truncationMode(.middle)
        }
    }

    @ViewBuilder
    private func result(_ r: Row) -> some View {
        switch r {
        case let .run(_, facts, logPath):
            HStack(spacing: 14) {
                ForEach(Array(facts.enumerated()), id: \.offset) { _, f in
                    Text("\(Text(f.value).monospacedDigit())\(Text(f.value.isEmpty ? f.label : " \(f.label)").foregroundStyle(.secondary))")
                        .lineLimit(1)
                        .help(f.tooltip)
                }
                Spacer(minLength: 0)
                if !logPath.isEmpty {
                    Button("Log File") { model.open(logPath) }
                        .buttonStyle(.link)
                        .help(logPath)
                }
            }
        case let .signIn(_, url):
            if let link = URL(string: url) {
                Link(url, destination: link)
            } else {
                Text(url).textSelection(.enabled)
            }
        case .error:
            EmptyView()
        case let .clean(_, _, outcome, _):
            SwiftUI.Group {
                switch outcome {
                case .removed:
                    Text("\(Text("→").foregroundStyle(.tertiary)) \(outcome.label)")
                case .kept, .wouldRemove:
                    Text(outcome.label).foregroundStyle(.secondary)
                }
            }
            .font(.callout.monospaced())
            .lineLimit(1)
        case let .asset(_, _, _, details, _):
            HStack(spacing: 12) {
                ForEach(Array(details.enumerated()), id: \.offset) { _, d in
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
