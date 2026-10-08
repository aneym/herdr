import Foundation

// Pure escaping algorithm: golden edge cases guard shell interpretation and
// item boundaries independently of the Cua scenario's real terminal paste.
@main
struct FileDropPathsCheck {
    static func main() {
        let cases: [([String], String)] = [
            (["/tmp/drop test dir"], #"/tmp/drop\ test\ dir "#),
            (["/tmp/\"'"], #"/tmp/\"\' "#),
            (["/tmp/$`!&;()"], #"/tmp/\$\`\!\&\;\(\) "#),
            (["/tmp/日本語😀"], "/tmp/日本語😀 "),
            (["/tmp/a b", "/tmp/c"], #"/tmp/a\ b /tmp/c "#),
            ([##"/tmp/\[]{}<>|*?~#"##], ##"/tmp/\\\[\]\{\}\<\>\|\*\?\~\# "##),
            ([], "")
        ]
        for (paths, expected) in cases {
            let actual = FileDropPaths.text(paths)
            precondition(actual == expected, "\(paths): \(actual.debugDescription) != \(expected.debugDescription)")
        }
        print("PASS: file drop escaping (\(cases.count) golden cases)")
    }
}
