import Foundation

@main struct Dump {
    struct Input: Decodable { let text: String; let markdown: Bool }
    struct Link: Encodable { let display: String; let target: String }
    struct Output: Encodable { let display: String; let links: [Link] }

    static func main() throws {
        let inputs = try JSONDecoder().decode([Input].self, from: FileHandle.standardInput.readDataToEndOfFile())
        let outputs = inputs.map { input in
            let text = input.markdown ? ChatLinks.markdown(input.text) : ChatLinks.plain(input.text)
            return Output(display: String(text.characters), links: text.runs.compactMap { run in
                guard let link = run.link else { return nil }
                return Link(display: String(text[run.range].characters), target: link.absoluteString)
            })
        }
        FileHandle.standardOutput.write(try JSONEncoder().encode(outputs))
    }
}
