# 分诊标签词汇表

工程技能以五个规范分诊角色说话，本文件把角色映射到本仓库实际使用的
标签字符串。当前全部使用默认字符串。

| 技能中的角色 | 本仓库标签 | 含义 |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | 待维护者分诊评估 |
| `needs-info` | `needs-info` | 等待报告者补充信息 |
| `ready-for-agent` | `ready-for-agent` | 规格完备，可交给 AFK agent 实施 |
| `ready-for-human` | `ready-for-human` | 需要人类实施 |
| `wontfix` | `wontfix` | 不予处理（GitHub 默认标签，语义一致） |

技能提到某个角色（如「打上 AFK-ready 分诊标签」）时，使用本表对应标签。
`ready-for-agent` 只表示没有前置依赖且规格完备，不表示已经开始实施或已获
推送授权（沿用仓库既有口径）。

标签改名时只需改右列，并同步在 GitHub 创建对应标签。
