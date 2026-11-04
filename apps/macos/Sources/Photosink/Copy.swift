import Foundation

enum WallClock {
    static let utc = TimeZone(identifier: "UTC")!

    static func date(_ seconds: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(seconds))
    }

    static func seconds(_ date: Date) -> Int64 {
        Int64(date.timeIntervalSince1970.rounded(.down))
    }

    static func text(_ seconds: Int64) -> String {
        date(seconds).formatted(Date.FormatStyle(date: .numeric, time: .standard, timeZone: utc))
    }
}

enum SidebarGroup: CaseIterable {
    case `import`, maintain

    var name: String {
        switch self {
        case .import: "Import"
        case .maintain: "Maintain"
        }
    }

    func contains(_ tab: Tab) -> Bool {
        switch tab {
        case .applePhotos, .oneDrive, .dropbox, .googleTakeout: self == .import
        case .consolidate, .clean, .dates: self == .maintain
        }
    }
}

extension Tab {
    var name: String {
        switch self {
        case .applePhotos: "Apple Photos"
        case .oneDrive: "OneDrive"
        case .dropbox: "Dropbox"
        case .googleTakeout: "Google Takeout"
        case .consolidate: "Consolidate"
        case .clean: "Clean"
        case .dates: "Dates"
        }
    }

    var hint: String {
        switch self {
        case .applePhotos: "Exports originals and metadata from the Photos library through PhotoKit."
        case .oneDrive: "Signs in with a device code the first time, then exports what changed since the last run."
        case .dropbox: "Signs in through the browser the first time, then exports what changed since the last run."
        case .googleTakeout: "Reads the archives in place; nothing needs extracting first."
        case .consolidate: "Moves everything already exported into the folders the current path template produces."
        case .clean:
            "Rows whose file is gone are forgotten; an asset left with no files loses its sidecar and is exported again next run. Sidecars without an asset are deleted. Dry run only lists."
        case .dates:
            "Matches folder, name or original filename. Pick a date per asset, then Apply stores it, rewrites metadata and moves the files."
        }
    }

    var argLabel: String? {
        switch self {
        case .applePhotos: "Library"
        case .oneDrive, .dropbox: "Folder"
        case .googleTakeout: "Takeout"
        case .dates: "Find"
        case .consolidate, .clean: nil
        }
    }

    func runLabel(_ args: RunArgs) -> String {
        switch self {
        case .applePhotos, .oneDrive, .dropbox, .googleTakeout: "Run"
        case .consolidate: "Consolidate"
        case .clean: args.dry ? "List" : "Remove"
        case .dates: args.arg.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Scan" : "Find"
        }
    }
}

extension Blocker {
    func text(for tab: Tab) -> String {
        switch self {
        case let .running(running): "\(running.name) is running."
        case .noExportFolder: "Choose a folder to export to first."
        case .argRequired: "Choose a \((tab.argLabel ?? "").lowercased()) first."
        case let .dropboxKeyMissing(path): "Set dropbox_app_key in \(path) to your Dropbox app's key."
        case .unavailable: "Apple Photos is only available on macOS"
        }
    }
}

extension Confirmation {
    var title: String {
        switch self {
        case .cleanDeletes: "Clean the export"
        }
    }

    var ok: String {
        switch self {
        case .cleanDeletes: "Delete"
        }
    }

    var message: String {
        switch self {
        case let .cleanDeletes(unreferenced):
            unreferenced
                ? "Delete sidecars whose files are gone, forget their rows, and delete every media file the state does not reference. This cannot be undone."
                : "Delete sidecars whose files are gone and forget their rows. This cannot be undone."
        }
    }
}

extension Status {
    var text: String {
        switch self {
        case .idle: "Idle"
        case let .running(tab): "Running \(tab.name)…"
        case .checkingDates: "Checking dates…"
        case .applyingDates: "Applying dates…"
        case let .done(errors): errors == 0 ? "Done" : "Done with \(errors.formatted()) errors"
        case .failed: "Failed"
        }
    }
}

extension StatKind {
    var label: String {
        switch self {
        case .exported: "exported"
        case .planned: "planned"
        case .duplicates: "duplicates"
        case .skipped: "skipped"
        case .filtered: "filtered"
        case .errors: "errors"
        case .moved: "moved"
        case .wouldMove: "would move"
        case .renamed: "renamed"
        case .unchanged: "unchanged"
        case .noMetadata: "no metadata"
        case .rowsChecked: "rows checked"
        case .deadRows: "dead rows"
        case .assetsReset: "assets reset"
        case .sidecarsRemoved: "sidecars removed"
        case .sidecarsToRemove: "sidecars to remove"
        case .unreferencedFiles: "unreferenced files"
        case .filesRemoved: "files removed"
        case .filesToRemove: "files to remove"
        }
    }
}

extension RowState {
    var label: String {
        switch self {
        case .exported: "Exported"
        case .moved: "Moved"
        case .redated: "Redated"
        case .duplicate: "Duplicate"
        case .metadata: "Metadata only"
        case .unchanged: "Nothing new"
        case .missing: "Missing file"
        case .abandoned: "Orphan sidecar"
        case .unclaimed: "Stray sidecar"
        case .unreferenced: "Unreferenced"
        case .failed: "Failed"
        case .other: "Other"
        }
    }
}

extension CleanReason {
    var label: String {
        switch self {
        case .missing: "missing file"
        case .abandoned: "orphan sidecar"
        case .unclaimed: "stray sidecar"
        case .unreferenced: "unreferenced"
        }
    }
}

extension CleanOutcome {
    var label: String {
        switch self {
        case .removed: "removed"
        case .kept: "kept"
        case .wouldRemove: "would remove"
        }
    }
}

extension RunTitle {
    var text: String {
        switch self {
        case let .run(tab, arg, dry, option, template):
            let tag = dry ? " (dry run)" : ""
            switch tab {
            case .consolidate: return "consolidate \(template)\(tag)"
            case .clean: return (option ? "clean + unreferenced files" : "clean") + tag
            case .dates: return arg.isEmpty ? "scan dates" : "find \(arg)"
            case .oneDrive: return (arg.isEmpty ? "onedrive (whole drive)" : "onedrive \(arg)") + tag
            case .dropbox: return (arg.isEmpty ? "dropbox (everything)" : "dropbox \(arg)") + tag
            case .applePhotos: return "apple-photos \(arg)\(tag)"
            case .googleTakeout: return "google-takeout \(arg)\(tag)"
            }
        case let .redate(count):
            return "redate \(count.formatted()) assets"
        }
    }
}

extension Fact {
    var value: String {
        switch self {
        case let .found(total, _, _), let .visibleToPhotoKit(total), let .changed(total): total.formatted()
        case let .inventory(files, _, _): files.formatted()
        case .warning, .listing, .reading: ""
        }
    }

    var label: String {
        switch self {
        case let .found(_, _, name): "assets in \(name)"
        case .visibleToPhotoKit: "visible to PhotoKit"
        case .changed: "new or changed"
        case let .inventory(_, sidecars, _): "files, \(sidecars.formatted()) sidecars"
        case let .warning(text): text
        case let .listing(_, name): "listing \(name)"
        case let .reading(_, name): "reading \(name)"
        }
    }

    var tooltip: String {
        switch self {
        case let .found(_, place, _): place
        case let .inventory(_, _, dir): dir
        case let .listing(path, _), let .reading(path, _): path
        case .visibleToPhotoKit, .changed, .warning: ""
        }
    }
}

extension SignIn {
    var tag: String {
        switch self {
        case .browser: "Approve access in your browser"
        case .deviceCode: "Sign in with code"
        }
    }
}

extension DateKey {
    static let cases: [DateKey] = [.create, .original, .modify]

    var label: String {
        switch self {
        case .create: "Create"
        case .original: "Original"
        case .modify: "Modify"
        }
    }
}

extension DatesView {
    var summary: String {
        var prefix = ""
        if scanning, let scan {
            prefix = "checked \(scan.checked.formatted())/\(scan.total.formatted()), "
        }
        return "\(prefix)\(rows.formatted()) \(query ? "matching" : "flagged"), \(chosen.formatted()) chosen"
    }

    var emptyText: String {
        if scanning { return "Checking…" }
        return query ? "Nothing matches." : "Every asset's folder date matches its file."
    }

    var applyLabel: String {
        applying ? "Applying…" : "Apply \(chosen.formatted())"
    }
}

extension DateSource {
    static let cases: [DateSource] = [.source, .file]
    static let hint = "falls back to the other, then the upload time"

    var label: String {
        switch self {
        case .source: "Source metadata first"
        case .file: "File EXIF first"
        }
    }
}

extension ZoneMode {
    static let cases: [ZoneMode] = [.asset, .local, .utc, .fixed]
    static let hint = "the zone the folder date is read in"
    static let offsetPlaceholder = "+02:00"

    var label: String {
        switch self {
        case .asset: "As recorded"
        case .local: "This Mac's zone"
        case .utc: "UTC"
        case .fixed: "Fixed offset"
        }
    }
}

extension Media {
    static let cases: [Media] = [.all, .photos, .videos]

    var label: String {
        switch self {
        case .all: "All"
        case .photos: "Photos"
        case .videos: "Videos"
        }
    }
}

extension Metadata {
    static let cases: [Metadata] = [.both, .embed, .sidecar]
    static let hint = "the XMP packet written into each file, to a .xmp beside it, or both"

    var label: String {
        switch self {
        case .both: "In files + sidecar"
        case .embed: "In files"
        case .sidecar: "Sidecar only"
        }
    }
}

extension TemplateKind {
    var name: String {
        switch self {
        case .folder: "path"
        case .filename: "filename"
        }
    }

    var placeholder: String {
        switch self {
        case .folder: "{take.year}/{take.month}"
        case .filename: "{stem}"
        }
    }

    var separator: Character {
        switch self {
        case .folder: "/"
        case .filename: "_"
        }
    }
}

extension FieldGroup {
    static let cases: [FieldGroup] = [.date, .place, .tags, .camera, .other]

    var name: String {
        switch self {
        case .date: "Date"
        case .place: "Place"
        case .tags: "Tags"
        case .camera: "Camera"
        case .other: "Other"
        }
    }
}

extension Field {
    var help: String {
        switch name {
        case "take.year": "year taken, 2019"
        case "take.month": "month taken, 05"
        case "take.day": "day taken, 03"
        case "take.date": "date taken, 2019-05-03"
        case "place.name": "place named by the source; {place.name|place.city} falls back"
        case "place.city": "city"
        case "place.state": "state or province"
        case "place.country": "country code, FR"
        case "keyword": "keyword n"
        case "album": "album n"
        case "label": "machine label n"
        case "people": "person n"
        case "camera.make": "Apple"
        case "camera.model": "iPhone 15 Pro"
        case "camera.lens": "lens model"
        case "title": "title"
        case "type": "photo or video"
        case "stem": "filename without extension"
        case "uuid": "asset id"
        default: ""
        }
    }
}

extension SettingsProblem {
    var text: String {
        switch self {
        case let .fromDate(value): "from date \(quoted(value)) is not YYYY-MM-DD"
        case let .toDate(value): "to date \(quoted(value)) is not YYYY-MM-DD"
        case let .offset(value): "date_zone \(quoted(value)) is not asset, local, utc or ±HH:MM"
        case let .unknownFields(template, names):
            "\(template.name) uses unknown \(names.count == 1 ? "field" : "fields") \(names.joined(separator: ", "))"
        case .fromAfterTo: "filter.from is after filter.to"
        case let .checkInterval(minutes): "Check interval \(minutes.formatted()) must be between 1 and \(1440.formatted()) minutes."
        }
    }
}

extension TemplateCheck {
    var problem: String? {
        guard !unknown.isEmpty else { return nil }
        let noun = unknown.count == 1 ? "Unknown field" : "Unknown fields"
        return "\(noun): \(unknown.joined(separator: ", ")). Pick from the list; it won't be saved until it's fixed."
    }
}

extension Preview {
    var rootText: String { root ?? "Export" }
    var dirText: String { dir.isEmpty ? "…" : dir }
    var sampleNote: String {
        exportedStem.map { "from \($0)" } ?? "sample photo until something is exported"
    }
}

extension Settings {
    var filtering: Bool {
        media != .all || favorites
            || !from.trimmingCharacters(in: .whitespaces).isEmpty
            || !to.trimmingCharacters(in: .whitespaces).isEmpty
            || !listItems(albums).isEmpty || !listItems(keywords).isEmpty
    }

    var filterHint: String {
        filtering ? "Only matching assets are exported. Consolidate and Dates ignore the filter." : "Everything is exported."
    }
}

extension CoreError {
    func text(for tab: Tab) -> String {
        switch self {
        case let .Blocked(blocker): blocker.text(for: tab)
        case let .Running(tab): "\(tab.name) is running."
        case .NothingChosen: "Nothing chosen."
        case let .InvalidSettings(problem): problem.text
        case let .Failed(detail): detail
        }
    }
}

private func quoted(_ s: String) -> String {
    "\"" + s.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"") + "\""
}

private func listItems(_ s: String) -> [String] {
    s.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
}

func message(_ error: Error, tab: Tab) -> String {
    (error as? CoreError)?.text(for: tab) ?? error.localizedDescription
}
