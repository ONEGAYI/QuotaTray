# Issue tracker: GitHub

本仓库的 issues 与规格以 GitHub Issues 为唯一载体（2026-09-10 起任务包不再落
`.scratch/` 本地文件；旧任务包 model-pricing-catalog 保留为历史存档）。
所有操作使用 `gh` CLI，仓库由 `git remote -v` 推断。

## 约定

- **创建 issue**：`gh issue create --title "..." --body "..."`，多行正文用 heredoc。
- **读 issue**：`gh issue view <number> --comments`。
- **列出**：`gh issue list --state open --json number,title,labels ...` 按需过滤。
- **评论**：`gh issue comment <number> --body "..."`。
- **标签**：`gh issue edit <number> --add-label "..."` / `--remove-label "..."`。
- **关闭**：`gh issue close <number> --comment "..."`。

GitHub 的 issue 与 PR 共用编号空间：裸 `#42` 可能是两者之一，用
`gh pr view 42` 与 `gh issue view 42` 互相兜底解析。

## 当技能说「发布到 issue tracker」时

创建 GitHub issue。规格（spec）发布为父 issue，工单发布为子 issue，
依赖用原生关系表达（见下）。工单标题沿用任务包编号前缀（如 `[T-09]`），
编号在历史任务包序列上延续。

## 任务包（spec + 工单）的发布形态

- **父子关系**：工单挂为规格 issue 的 sub-issue：
  `gh api --method POST repos/<owner>/<repo>/issues/<parent>/sub_issues -F sub_issue_id=<child-db-id>`
  （db-id 用 `gh api repos/<owner>/<repo>/issues/<n> --jq .id` 取，不是 `#编号`）。
- **阻塞边**：用 GitHub 原生 issue dependencies：
  `gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`。
  阻塞状态看 `issue_dependencies_summary.blocked_by`（仅计 open 的 blocker）。
- **frontier**：列出规格的 open 子票，剔除有 open blocker 或已有 assignee 的，
  剩余即可抓取。抓取动作：`gh issue edit <n> --add-assignee @me`。
- **完结**：实施 PR 在正文用 `Close #<spec>`、`Close #<工单>` 关闭对应 issue；
  受时间门约束的跟进票（如 T-11）保持 open，不随 PR 关闭。

## PR 作为分诊入口

**PRs as a request surface: no。** 外部 PR 不进入分诊队列；如将来需要，
把本标志改为 yes 并补充 `gh pr` 等价操作。
