/// Bounded URL-opening trace for the dev test hook, independent of attention state.
final class OpenedLinks {
    static let shared = OpenedLinks()
    private(set) var urls: [String] = []

    func record(_ url: String) {
        urls.append(url)
        if urls.count > 10 { urls.removeFirst(urls.count - 10) }
    }
}
