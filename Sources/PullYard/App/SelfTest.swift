import Foundation

enum SelfTest {
    static func run() {
        guard let token = ProcessInfo.processInfo.environment["PULLYARD_TOKEN"] else {
            print("PULLYARD_TOKEN not set")
            exit(2)
        }
        let api = GitHubAPI(token: token)
        let done = DispatchSemaphore(value: 0)
        var failures = 0
        Task.detached {
            func step(_ name: String, _ op: () async throws -> String) async {
                do {
                    print("OK   \(name): \(try await op())")
                } catch {
                    failures += 1
                    print("FAIL \(name): \(error.localizedDescription)")
                }
            }
            var samplePR: ItemRef?
            var sampleIssue: ItemRef?
            await step("user") { let (u, s) = try await api.currentUser(); return "\(u.login) scopes=\(s)" }
            await step("badges") { let b = try await api.badges(); return "rr=\(b.rr.issueCount) mine=\(b.mine.issueCount)" }
            for q in ["is:pr author:@me is:closed archived:false sort:updated-desc", "is:pr review-requested:@me archived:false sort:updated-desc", "is:pr author:@me sort:updated-desc"] {
                await step("search \(q)") {
                    let r = try await api.search(q)
                    samplePR = r.nodes.first?.ref
                    return "count=\(r.issueCount) first=\(r.nodes.first?.ref.title ?? "-")"
                }
            }
            await step("search issues") {
                let r = try await api.search("is:issue involves:@me sort:updated-desc")
                sampleIssue = r.nodes.first?.ref
                return "count=\(r.issueCount)"
            }
            if let ref = ProcessInfo.processInfo.environment["PULLYARD_PR"].flatMap({ GitHubURL.parse($0) }).flatMap({ r -> ItemRef? in if case .pr(let x) = r { return x } else { return nil } }) {
                samplePR = ref
            }
            if let pr = samplePR {
                await step("pr \(pr.title)") {
                    let r = try await api.pullRequest(pr)
                    guard let p = r.repository?.pullRequest else { return "not a PR" }
                    let html = ConversationHTML.render(pr: p)
                    let fix = FixPromptExtractor.latest(in: p)
                    let unresolved = p.reviewThreads.nodes.filter { !$0.isResolved }
                    let composed = FixPrompt.build(pr: p, repo: pr.repo, threads: unresolved, general: FixPrompt.generalFeedback(p), options: FixOptions())
                    let folder = await LocalRepoStore.detect(pr.repo) ?? "-"
                    if ProcessInfo.processInfo.environment["PULLYARD_SHOWFIX"] != nil {
                        print("----- FOUND -----\n" + (fix.map { String($0.text.prefix(300)) + "\n…\n" + String($0.text.suffix(200)) } ?? "none"))
                        print("----- COMPOSED -----\n" + String(composed.prefix(1200)))
                    }
                    return "state=\(p.state) timeline=\(p.timelineItems.nodes.count) threads=\(p.reviewThreads.nodes.count) unresolved=\(unresolved.count) html=\(html.count) fixPrompt=\(fix.map { "@\($0.author) \($0.text.count)ch \(Fmt.rel($0.date))" } ?? "none") composed=\(composed.count)ch folder=\(folder)"
                }
                await step("pulse") {
                    let full = try await api.pullRequest(pr)
                    let light = try await api.pullRequestPulse(pr)
                    guard let known = full.pulse, let light else { return "missing pulse" }
                    return "head=\(light.headRefOid.prefix(7)) unchanged=\(known == light)"
                }
                await step("conflicts") {
                    let r = try await api.pullRequest(pr)
                    guard let p = r.repository?.pullRequest else { return "not a PR" }
                    guard MergeConflicts.applies(p) else { return "none (\(p.mergeable) \(p.mergeStateStatus ?? "-"))" }
                    guard let folder = await LocalRepoStore.detect(pr.repo) else { return "\(ConflictState.noClone)" }
                    let state = MergeConflicts.scan(folder: folder, repo: pr.repo, number: p.number, base: p.baseRefName, baseOid: p.baseTipOid, headOid: p.headRefOid)
                    let html = ConversationHTML.render(pr: p, conflicts: state, repo: pr.repo)
                    let prompt = ConflictPrompt.build(pr: p, repo: pr.repo, files: state.files, push: true)
                    if ProcessInfo.processInfo.environment["PULLYARD_SHOWFIX"] != nil { print("----- CONFLICT PROMPT -----\n" + prompt) }
                    return "\(state) html=\(html.contains("Resolve Conflicts with Claude Code")) prompt=\(prompt.count)ch"
                }
                await step("files") {
                    let f = try await api.pullRequestFiles(pr)
                    let lines = f.map(DiffFile.init).reduce(0) { $0 + $1.lines.count }
                    return "files=\(f.count) lines=\(lines)"
                }
            }
            if let pr = samplePR {
                await step("repo PRs \(pr.repo.full)") {
                    let open = try await api.repoPullRequests(pr.repo, states: ["OPEN"])
                    let merged = try await api.repoPullRequests(pr.repo, states: ["MERGED"])
                    let branch = ProcessInfo.processInfo.environment["PULLYARD_BRANCH"] ?? "main"
                    let byBranch = try await api.repoPullRequests(pr.repo, headRefName: branch, states: ["OPEN", "MERGED", "CLOSED"], first: 5)
                    return "open=\(open.count) [\(open.prefix(3).map { "#\($0.number)" }.joined(separator: ","))] merged=\(merged.count) [\(merged.prefix(3).map { "#\($0.number)" }.joined(separator: ","))] branch(\(branch))=\(byBranch.map { "#\($0.number) \($0.prState ?? "")" })"
                }
            }
            if let issue = sampleIssue {
                await step("issue \(issue.title)") {
                    let r = try await api.issue(issue)
                    return "type=\(r.repository?.issueOrPullRequest?.typename ?? "-") timeline=\(r.repository?.issueOrPullRequest?.issue?.timelineItems.nodes.count ?? -1)"
                }
            }
            await step("repos") {
                let r = try await api.repos(after: nil)
                let back = try JSONDecoder().decode(ListSnapshot<[RepoSummary]>.self, from: JSONEncoder().encode(ListSnapshot(saved: Date(), value: r.nodes)))
                return "total=\(r.totalCount) first=\(r.nodes.first?.nameWithOwner ?? "-") cached=\(back.value.count)"
            }
            await step("list cache") {
                let r = try await api.search("is:pr involves:@me sort:updated-desc")
                let back = try JSONDecoder().decode(ListSnapshot<[SearchItem]>.self, from: JSONEncoder().encode(ListSnapshot(saved: Date(), value: r.nodes)))
                let same = zip(r.nodes, back.value).allSatisfy { $0.id == $1.id && $0.updatedAt == $1.updatedAt && $0.ciState == $1.ciState }
                return "items=\(back.value.count) roundTrip=\(same && back.value.count == r.nodes.count)"
            }
            await step("projects") {
                let p = try await api.projects()
                if let first = p.first {
                    let (d, items) = try await api.project(first.id)
                    return "count=\(p.count) first=\(d.title) fields=\(d.fields.nodes.compactMap(\.field).count) items=\(items.count)"
                }
                return "count=0"
            }
            await step("notifications") {
                let n = try await api.notifications(all: true)
                let back = try JSONDecoder().decode(ListSnapshot<[NotificationThread]>.self, from: JSONEncoder().encode(ListSnapshot(saved: Date(), value: n)))
                return "all=\(n.count) cached=\(back.value.count)"
            }
            done.signal()
        }
        done.wait()
        exit(failures == 0 ? 0 : 1)
    }
}
