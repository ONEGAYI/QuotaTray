//! 完整配置的跨机器迁移格式。
//!
//! 普通 `config.json` 的机器主密钥仍只存在于系统凭据库。显式导出时，先把每条
//! 凭据转写到本次导出的迁移密钥，再用同一迁移密钥整体加密配置，写入私有
//! 二进制容器。
//!
//! 容器版本：v1 明文载荷为 `AppConfig`（历史版本，仍可导入）；v2 起为
//! `{ config, history, usage_comparison_series? }` 信封，随配置携带历史走势数据
//! 与可选的使用统计比较组合（均不含凭据）；v3 起头部新增档位字段，导出双档：
//!
//! - **便捷档**：一次性 32 字节随机迁移密钥随包携带，导入零门槛；迁移密钥
//!   与密文同包，敏感级别等同明文凭据（v1/v2 容器恒为此档语义）。
//! - **密码档**：用户口令经 Argon2id 派生密钥加密，派生密钥绝不写入容器
//!   任何字节；KDF 参数（算法/内存/迭代/并行度/盐）与信封 GCM nonce 写入
//!   头部自描述，旧包永远可用头部参数重派生解密。
//!
//! 新版本只产 v3；仅支持低版本的旧二进制读到更高版本会拒绝（版本拒绝规则
//! 延续）。信封 AAD 按版本+档位细分，v1/v2 既有 AAD 保持不变。
//!
//! 导入双模（`ImportOptions::strategy`，策略在写入层生效）：合并（默认）
//! = 不丢本机任何东西——条目按 id 并集（同 id 本机为准）、比较组合按
//! (provider_id, window_key) 并集（本机为准、超 4 条截断）、历史幂等合并；
//! 覆盖 = 完全变成备份——配置与比较组合整体替换、历史单事务清空重插
//! （`HistoryStore::replace_rows`）。既有无 options 的旧导入入口维持
//! 「整体替换」现状语义（显式按覆盖委托，不受默认合并影响）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use super::AppConfig;
use crate::history::HistoryExportRow;
use crate::vault::{Vault, VaultError};

/// QuotaTray 配置迁移文件的推荐扩展名（不含点）。
pub const CONFIG_EXPORT_EXTENSION: &str = "qtray-export";

const MAGIC: &[u8; 8] = b"QTRAYCFG";
const FORMAT_VERSION_V1: u16 = 1;
const FORMAT_VERSION_V2: u16 = 2;
const FORMAT_VERSION_V3: u16 = 3;
const TRANSFER_KEY_LEN: usize = 32;
const VERSION_OFFSET: usize = MAGIC.len();
const KEY_OFFSET: usize = VERSION_OFFSET + 2;
const LENGTH_OFFSET: usize = KEY_OFFSET + TRANSFER_KEY_LEN;
const HEADER_LEN: usize = LENGTH_OFFSET + 4;
/// 迁移包大小上限（16 MiB）；公开给调用端在整读文件前做预检
/// （[`precheck_transfer_file_size`]），避免误选的大文件先占满内存。
pub const MAX_EXPORT_SIZE: usize = 16 * 1024 * 1024;
const ENVELOPE_AAD_V1: &str = "quotatray-config-export:v1";
const ENVELOPE_AAD_V2: &str = "quotatray-config-export:v2";
const ENVELOPE_AAD_V3_CONVENIENT: &str = "quotatray-config-export:v3-convenient";
const ENVELOPE_AAD_V3_PASSWORD: &str = "quotatray-config-export:v3-password";

/// 备份口令的最小字符数（按 `char` 计）。
const MIN_PASSWORD_CHARS: usize = 8;

// v3 头部公共前缀：[0..8) 魔数、[8..10) 版本、[10..11) 档位，其后按档位分叉。
const MODE_OFFSET: usize = VERSION_OFFSET + 2;
const MODE_CONVENIENT: u8 = 0;
const MODE_PASSWORD: u8 = 1;

// v3 便捷档：档位字节后接 32B 随机迁移密钥 + u32 载荷长度（与 v1/v2 的
// 「密钥随包」布局语义一致，仅前置档位字节）。
const V3_CONVENIENT_KEY_OFFSET: usize = MODE_OFFSET + 1;
const V3_CONVENIENT_LENGTH_OFFSET: usize = V3_CONVENIENT_KEY_OFFSET + TRANSFER_KEY_LEN;
const V3_CONVENIENT_HEADER_LEN: usize = V3_CONVENIENT_LENGTH_OFFSET + 4;

// v3 密码档：档位字节后接自描述 KDF 头（算法/内存 KiB/迭代/并行度/32B 盐）
// + 12B 信封 GCM nonce + u32 载荷长度；派生密钥绝不写入容器。
const KDF_ALG_ARGON2ID: u32 = 1;
const KDF_ALGORITHM_OFFSET: usize = MODE_OFFSET + 1;
const KDF_MEMORY_OFFSET: usize = KDF_ALGORITHM_OFFSET + 4;
const KDF_TIME_OFFSET: usize = KDF_MEMORY_OFFSET + 4;
const KDF_PARALLELISM_OFFSET: usize = KDF_TIME_OFFSET + 4;
const KDF_SALT_OFFSET: usize = KDF_PARALLELISM_OFFSET + 4;
const KDF_SALT_LEN: usize = 32;
const CIPHER_NONCE_OFFSET: usize = KDF_SALT_OFFSET + KDF_SALT_LEN;
const CIPHER_NONCE_LEN: usize = crate::vault::NONCE_LEN;
const V3_PASSWORD_LENGTH_OFFSET: usize = CIPHER_NONCE_OFFSET + CIPHER_NONCE_LEN;
const V3_PASSWORD_HEADER_LEN: usize = V3_PASSWORD_LENGTH_OFFSET + 4;

/// Argon2id 导出默认参数（OWASP 交互式推荐档：19 MiB 内存 / 2 迭代 / 1 并行）。
const ARGON2_DEFAULT_MEMORY_KIB: u32 = 19456;
const ARGON2_DEFAULT_TIME_COST: u32 = 2;
const ARGON2_DEFAULT_PARALLELISM: u32 = 1;
/// 导入侧接受的 KDF 参数上限（拒绝恶意容器头部触发的 KDF DoS）；
/// 导出默认参数远低于上限，为未来升级留空间。
const KDF_MAX_MEMORY_KIB: u32 = 1 << 20;
const KDF_MAX_TIME_COST: u32 = 4096;
const KDF_MAX_PARALLELISM: u32 = 64;

/// 使用统计中持久化的一条 Provider + 窗口组合。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct UsageComparisonSeries {
    /// Provider 的稳定配置 id。
    pub provider_id: String,
    /// 历史库中的模型/额度窗口键。
    pub window_key: String,
    /// 固定颜色槽，合法范围为 `0..MAX_USAGE_COMPARISON_SERIES`。
    pub color_slot: u8,
}

/// 同屏比较的最大曲线数，也是合法色槽数量。
pub const MAX_USAGE_COMPARISON_SERIES: usize = 4;

/// 归一化跨端共享的比较组合：裁剪键、按顺序去重、限制四条，并修复色槽。
pub fn sanitize_usage_comparison_series(
    items: Vec<UsageComparisonSeries>,
) -> Vec<UsageComparisonSeries> {
    let mut normalized = Vec::with_capacity(items.len().min(MAX_USAGE_COMPARISON_SERIES));
    let mut keys = HashSet::new();
    let mut slots = HashSet::new();
    for mut item in items {
        item.provider_id = item.provider_id.trim().to_owned();
        item.window_key = item.window_key.trim().to_owned();
        if item.provider_id.is_empty()
            || item.window_key.is_empty()
            || !keys.insert((item.provider_id.clone(), item.window_key.clone()))
        {
            continue;
        }
        if usize::from(item.color_slot) >= MAX_USAGE_COMPARISON_SERIES
            || !slots.insert(item.color_slot)
        {
            let Some(slot) = (0..MAX_USAGE_COMPARISON_SERIES)
                .map(|slot| u8::try_from(slot).expect("four color slots fit u8"))
                .find(|slot| !slots.contains(slot))
            else {
                break;
            };
            item.color_slot = slot;
            slots.insert(slot);
        }
        normalized.push(item);
        if normalized.len() == MAX_USAGE_COMPARISON_SERIES {
            break;
        }
    }
    normalized
}

/// 合并模的比较组合并集：本机组合全保留（保序保色槽），备份仅补本机
/// 没有的 (provider_id, window_key) 键，整体再经 [`sanitize_usage_comparison_series`]
/// 修复色槽冲突并维持 4 条上限。
///
/// 计数口径：`series_skipped` = 备份中与本机同键（以本机为准）的数量；
/// `series_added` = 备份中成功并入最终列表的数量；被 4 条上限截断的
/// 备份键不计入任一计数。
pub fn merge_usage_comparison_series(
    local: &[UsageComparisonSeries],
    incoming: &[UsageComparisonSeries],
) -> (Vec<UsageComparisonSeries>, ImportCounts) {
    let local_series = sanitize_usage_comparison_series(local.to_vec());
    let local_keys: HashSet<(String, String)> = local_series
        .iter()
        .map(|item| (item.provider_id.clone(), item.window_key.clone()))
        .collect();
    let mut merged = local_series;
    let mut counts = ImportCounts::default();
    for item in sanitize_usage_comparison_series(incoming.to_vec()) {
        let key = (item.provider_id.clone(), item.window_key.clone());
        if local_keys.contains(&key) {
            counts.series_skipped += 1;
        } else if merged.len() < MAX_USAGE_COMPARISON_SERIES {
            merged.push(item);
            counts.series_added += 1;
        }
        // 已达 4 条上限：剩余备份键被截断，不计入任一计数
    }
    (sanitize_usage_comparison_series(merged), counts)
}

/// 迁移容器的档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferMode {
    /// 便捷档：一次性随机迁移密钥随包携带（v1/v2 容器恒为此档语义）。
    Convenient,
    /// 密码档：口令经 Argon2id 派生密钥加密，派生密钥绝不写入容器。
    Password,
}

/// 只读识别出的迁移容器元信息（不解密、不验证密码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferContainerInfo {
    /// 容器格式版本（1/2/3）。
    pub version: u16,
    /// 档位；v1/v2 容器恒报便捷档。
    pub mode: TransferMode,
}

/// 导出档位选项。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum ExportOptions {
    /// 便捷档：一次性 32 字节随机迁移密钥随包携带，包的保密等级等同明文凭据。
    Convenient,
    /// 密码档：口令经 Argon2id 派生密钥加密，派生密钥绝不写入容器；
    /// 口令至少 8 个字符（按 `char` 计），不足确定性拒绝。
    Password { password: String },
}

/// Debug 输出永不携带口令：口令是密码档唯一的秘密材料，任何 `{:?}`
/// 打点（日志、断言失败、panic message）都不得泄漏。
impl std::fmt::Debug for ExportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportOptions::Convenient => f.write_str("Convenient"),
            ExportOptions::Password { .. } => f.write_str("Password { password: <redacted> }"),
        }
    }
}

/// 导入策略：在写入层（`import_config_to_path*` 家族）决定备份如何与
/// 本机数据合并。
///
/// 设计哲学：**合并 = 不丢本机任何东西；覆盖 = 完全变成备份**。
/// 交互与确认由调用端（CLI/GUI）负责，core 不弹交互。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportStrategy {
    /// 合并（默认，保守）：条目按 id 并集、同 id 冲突以本机为准（备份条目
    /// 跳过，无需凭据转写）；比较组合按 (provider_id, window_key) 并集、
    /// 冲突本机为准、超 4 条截断；历史幂等合并。
    #[default]
    Merge,
    /// 覆盖：配置与比较组合整体替换；历史单事务清空本机后重插备份行
    /// （`HistoryStore::replace_rows`）。
    Overwrite,
}

/// 导入选项。
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ImportOptions {
    /// 密码档容器的备份口令；便捷档容器忽略此字段。
    pub password: Option<String>,
    /// 导入策略；缺省（含旧序列化形态与 `Default`）为合并（保守）。
    #[serde(default)]
    pub strategy: ImportStrategy,
}

/// Debug 输出永不携带口令（与 [`ExportOptions`] 同一卫生标准）。
impl std::fmt::Debug for ImportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportOptions")
            .field("password", &self.password.as_deref().map(|_| "<redacted>"))
            .field("strategy", &self.strategy)
            .finish()
    }
}

/// v2 容器的明文信封；可选字段缺省表示旧包未携带。
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ExportEnvelope {
    config: AppConfig,
    #[serde(default)]
    history: Option<Vec<HistoryExportRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage_comparison_series: Option<Vec<UsageComparisonSeries>>,
}

/// 解码后的迁移包：已转写凭据的配置 + 可选的历史数据（未合并进库）。
#[derive(Debug)]
pub struct TransferBundle {
    pub config: AppConfig,
    /// v2 容器携带的历史行；v1 容器或未携带时为 `None`。
    pub history: Option<Vec<HistoryExportRow>>,
    /// GUI/CLI settings.json 中的使用统计比较组合；旧包未携带时为 `None`。
    pub usage_comparison_series: Option<Vec<UsageComparisonSeries>>,
    /// 按策略应用到本机配置的生效计数；纯解码入口不接触本机状态，恒为零值。
    pub counts: ImportCounts,
}

/// 导入按策略应用的生效计数（新增/跳过）。
///
/// - 合并模：`*_added` = 备份中新并入本机的数量；`*_skipped` = 备份中因
///   与本机同 id/同键冲突而以本机为准跳过的数量。
/// - 覆盖模：整体替换，`*_added` = 生效的备份数量，`*_skipped` = 0
///   （覆盖无跳过概念）。
///
/// 条目维度由写入层入口（[`import_config_to_path_with_options`] 及其
/// 兼容档）对 config.json 的应用结果填充；比较组合维度仅在覆盖模由写入层
/// 填充（整体替换、包内即生效量），合并模的组合并集发生在调用端的
/// settings.json 层，计数由 [`merge_usage_comparison_series`] 返回。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImportCounts {
    /// 新增并入（合并）或整体生效（覆盖）的条目数。
    pub providers_added: usize,
    /// 合并模因同 id 冲突跳过的备份条目数；覆盖模恒 0。
    pub providers_skipped: usize,
    /// 新增并入（合并，由 `merge_usage_comparison_series` 填充）或整体
    /// 生效（覆盖，写入层填充）的比较组合数。
    pub series_added: usize,
    /// 合并模因同键冲突跳过的备份组合数（由 `merge_usage_comparison_series`
    /// 填充）；覆盖模恒 0。
    pub series_skipped: usize,
}

/// 已解析的容器头部（不含任何密码学验证，供 inspect 与导入共用）。
struct ContainerHeader<'a> {
    version: u16,
    mode: TransferMode,
    /// 便捷档：随包携带的迁移密钥；密码档为 `None`。
    transfer_key: Option<&'a [u8]>,
    /// 密码档：头部自描述的 KDF 参数与信封 nonce；便捷档为 `None`。
    password_params: Option<PasswordHeaderParams>,
    /// 信封加密 AAD（按版本+档位细分）。
    aad: &'static str,
    /// 载荷起始偏移。
    payload_offset: usize,
}

/// v3 密码档头部自描述的 Argon2id 参数与信封 GCM nonce。
struct PasswordHeaderParams {
    algorithm: u32,
    memory_kib: u32,
    time_cost: u32,
    parallelism: u32,
    salt: [u8; KDF_SALT_LEN],
    nonce: [u8; CIPHER_NONCE_LEN],
}

/// 解析容器头部：校验魔数、版本、档位、长度一致性，不做解密。
fn parse_header(bytes: &[u8]) -> Result<ContainerHeader<'_>, ConfigTransferError> {
    if bytes.len() > MAX_EXPORT_SIZE {
        return Err(ConfigTransferError::TooLarge);
    }
    if bytes.len() < KEY_OFFSET {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "文件头或载荷不完整",
        });
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "魔数不匹配",
        });
    }

    let version = u16::from_be_bytes(
        bytes[VERSION_OFFSET..KEY_OFFSET]
            .try_into()
            .expect("fixed version field"),
    );
    match version {
        FORMAT_VERSION_V1 | FORMAT_VERSION_V2 => parse_legacy_header(bytes, version),
        FORMAT_VERSION_V3 => parse_v3_header(bytes),
        _ => Err(ConfigTransferError::UnsupportedVersion { version }),
    }
}

/// v1/v2 头部：32B 随机迁移密钥 + u32 载荷长度（档位概念上恒为便捷档）。
fn parse_legacy_header(
    bytes: &[u8],
    version: u16,
) -> Result<ContainerHeader<'_>, ConfigTransferError> {
    if bytes.len() < HEADER_LEN {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "文件头或载荷不完整",
        });
    }
    let payload_len = u32::from_be_bytes(
        bytes[LENGTH_OFFSET..HEADER_LEN]
            .try_into()
            .expect("fixed length field"),
    ) as usize;
    verify_payload_len(bytes, HEADER_LEN, payload_len)?;
    Ok(ContainerHeader {
        version,
        mode: TransferMode::Convenient,
        transfer_key: Some(&bytes[KEY_OFFSET..LENGTH_OFFSET]),
        password_params: None,
        aad: if version == FORMAT_VERSION_V1 {
            ENVELOPE_AAD_V1
        } else {
            ENVELOPE_AAD_V2
        },
        payload_offset: HEADER_LEN,
    })
}

/// v3 头部：先读档位字节，再按档位解析对应布局。
fn parse_v3_header(bytes: &[u8]) -> Result<ContainerHeader<'_>, ConfigTransferError> {
    if bytes.len() < V3_CONVENIENT_HEADER_LEN {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "文件头或载荷不完整",
        });
    }
    match bytes[MODE_OFFSET] {
        MODE_CONVENIENT => {
            let payload_len = u32::from_be_bytes(
                bytes[V3_CONVENIENT_LENGTH_OFFSET..V3_CONVENIENT_HEADER_LEN]
                    .try_into()
                    .expect("fixed length field"),
            ) as usize;
            verify_payload_len(bytes, V3_CONVENIENT_HEADER_LEN, payload_len)?;
            Ok(ContainerHeader {
                version: FORMAT_VERSION_V3,
                mode: TransferMode::Convenient,
                transfer_key: Some(&bytes[V3_CONVENIENT_KEY_OFFSET..V3_CONVENIENT_LENGTH_OFFSET]),
                password_params: None,
                aad: ENVELOPE_AAD_V3_CONVENIENT,
                payload_offset: V3_CONVENIENT_HEADER_LEN,
            })
        }
        MODE_PASSWORD => {
            if bytes.len() < V3_PASSWORD_HEADER_LEN {
                return Err(ConfigTransferError::InvalidFormat {
                    reason: "文件头或载荷不完整",
                });
            }
            let algorithm = u32::from_be_bytes(
                bytes[KDF_ALGORITHM_OFFSET..KDF_MEMORY_OFFSET]
                    .try_into()
                    .expect("fixed algorithm field"),
            );
            let memory_kib = u32::from_be_bytes(
                bytes[KDF_MEMORY_OFFSET..KDF_TIME_OFFSET]
                    .try_into()
                    .expect("fixed memory field"),
            );
            let time_cost = u32::from_be_bytes(
                bytes[KDF_TIME_OFFSET..KDF_PARALLELISM_OFFSET]
                    .try_into()
                    .expect("fixed time field"),
            );
            let parallelism = u32::from_be_bytes(
                bytes[KDF_PARALLELISM_OFFSET..KDF_SALT_OFFSET]
                    .try_into()
                    .expect("fixed parallelism field"),
            );
            let salt: [u8; KDF_SALT_LEN] = bytes[KDF_SALT_OFFSET..CIPHER_NONCE_OFFSET]
                .try_into()
                .expect("fixed salt field");
            let nonce: [u8; CIPHER_NONCE_LEN] = bytes
                [CIPHER_NONCE_OFFSET..V3_PASSWORD_LENGTH_OFFSET]
                .try_into()
                .expect("fixed nonce field");
            let payload_len = u32::from_be_bytes(
                bytes[V3_PASSWORD_LENGTH_OFFSET..V3_PASSWORD_HEADER_LEN]
                    .try_into()
                    .expect("fixed length field"),
            ) as usize;
            verify_payload_len(bytes, V3_PASSWORD_HEADER_LEN, payload_len)?;
            Ok(ContainerHeader {
                version: FORMAT_VERSION_V3,
                mode: TransferMode::Password,
                transfer_key: None,
                password_params: Some(PasswordHeaderParams {
                    algorithm,
                    memory_kib,
                    time_cost,
                    parallelism,
                    salt,
                    nonce,
                }),
                aad: ENVELOPE_AAD_V3_PASSWORD,
                payload_offset: V3_PASSWORD_HEADER_LEN,
            })
        }
        _ => Err(ConfigTransferError::InvalidFormat {
            reason: "未知的容器档位",
        }),
    }
}

/// 校验「头部 + 载荷长度」与总长精确一致，且不超 16 MiB（两档同限）。
fn verify_payload_len(
    bytes: &[u8],
    header_len: usize,
    payload_len: usize,
) -> Result<(), ConfigTransferError> {
    let expected_len = header_len
        .checked_add(payload_len)
        .ok_or(ConfigTransferError::TooLarge)?;
    if expected_len > MAX_EXPORT_SIZE {
        return Err(ConfigTransferError::TooLarge);
    }
    if bytes.len() != expected_len {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "载荷长度不符或存在尾随数据",
        });
    }
    Ok(())
}

/// 配置迁移编码、认证、凭据转写或文件读写错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigTransferError {
    #[error("配置迁移包格式无效：{reason}")]
    InvalidFormat { reason: &'static str },
    #[error("不支持的配置迁移包版本：{version}")]
    UnsupportedVersion { version: u16 },
    #[error("配置迁移包超过 16 MiB 上限")]
    TooLarge,
    #[error("配置迁移包读取失败：{0}")]
    Read(#[source] std::io::Error),
    #[error("配置迁移包写入失败：{0}")]
    Write(#[source] std::io::Error),
    #[error("配置迁移内容序列化失败：{0}")]
    Serialize(#[source] serde_json::Error),
    #[error("配置迁移内容解析失败：{0}")]
    Parse(#[source] serde_json::Error),
    #[error("配置迁移凭据处理失败：{0}")]
    Vault(#[from] VaultError),
    #[error("导入配置保存失败：{0}")]
    Save(#[from] super::ConfigError),
    /// 合并模读取本机配置失败（如 JSON 损坏）；不复用 `Save` 的
    /// `#[from]`（同源类型只能有一个 from 转换），调用点显式 map_err。
    #[error("导入配置读取失败：{0}")]
    Load(#[source] super::ConfigError),
    #[error("备份密码至少需要 8 个字符")]
    PasswordTooShort,
    #[error("迁移包受密码保护，请在导入时提供备份密码")]
    PasswordRequired,
    #[error("备份密码错误或包已损坏")]
    PasswordAuthFailed,
    #[error("备份密码密钥派生失败：{0}")]
    Kdf(#[source] argon2::Error),
}

/// 将完整配置（可选携带历史数据）编码为携带一次性迁移密钥的私有认证容器。
///
/// 保留 v2 历史迁移阶段的公开签名，便捷档默认；需要携带统计比较组合或
/// 选择密码档时使用 [`export_config_with_options`]。
pub fn export_config(
    config: &AppConfig,
    source_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
) -> Result<Vec<u8>, ConfigTransferError> {
    export_config_with_usage(config, source_vault, history, None)
}

/// 将完整配置、可选历史与使用统计组合编码为私有认证容器（便捷档默认）。
///
/// 所有已配置凭据必须能被 `source_vault` 解密，否则整次导出失败。
/// 始终产 v3 信封容器；历史与比较组合均可选择不携带。
/// 密码档导出使用 [`export_config_with_options`]。
pub fn export_config_with_usage(
    config: &AppConfig,
    source_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
    usage_comparison_series: Option<&[UsageComparisonSeries]>,
) -> Result<Vec<u8>, ConfigTransferError> {
    export_config_with_options(
        config,
        source_vault,
        history,
        usage_comparison_series,
        &ExportOptions::Convenient,
    )
}

/// 将完整配置、可选历史与使用统计组合按指定档位编码为私有认证容器。
///
/// 便捷档产 v3 mode=0 容器（32B 随机迁移密钥随包，语义与 v1/v2 一致）；
/// 密码档产 v3 mode=1 容器：口令经 Argon2id 派生密钥加密，KDF 参数与信封
/// nonce 写入头部自描述，派生密钥绝不写入容器任何字节。口令不足 8 个字符
/// 时确定性拒绝（[`ConfigTransferError::PasswordTooShort`]）。
pub fn export_config_with_options(
    config: &AppConfig,
    source_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
    usage_comparison_series: Option<&[UsageComparisonSeries]>,
    options: &ExportOptions,
) -> Result<Vec<u8>, ConfigTransferError> {
    match options {
        ExportOptions::Convenient => {
            let (transfer_vault, transfer_key) = Vault::transient()?;
            let serialized = assemble_envelope(
                config,
                source_vault,
                &transfer_vault,
                history,
                usage_comparison_series,
            )?;
            let sealed = transfer_vault.encrypt(&serialized, ENVELOPE_AAD_V3_CONVENIENT)?;
            let mut prefix = Vec::with_capacity(V3_CONVENIENT_HEADER_LEN);
            prefix.extend_from_slice(MAGIC);
            prefix.extend_from_slice(&FORMAT_VERSION_V3.to_be_bytes());
            prefix.push(MODE_CONVENIENT);
            prefix.extend_from_slice(transfer_key.as_slice());
            append_payload(&prefix, sealed.as_bytes())
        }
        ExportOptions::Password { password } => {
            if password.chars().count() < MIN_PASSWORD_CHARS {
                return Err(ConfigTransferError::PasswordTooShort);
            }
            let salt = crate::vault::random_salt();
            let derived = derive_transfer_key(
                password,
                KDF_ALG_ARGON2ID,
                ARGON2_DEFAULT_MEMORY_KIB,
                ARGON2_DEFAULT_TIME_COST,
                ARGON2_DEFAULT_PARALLELISM,
                &salt,
            )?;
            let transfer_vault = Vault::from_master_key(&derived[..])?;
            let serialized = assemble_envelope(
                config,
                source_vault,
                &transfer_vault,
                history,
                usage_comparison_series,
            )?;
            let (nonce, sealed) = transfer_vault
                .seal_with_random_nonce(serialized.as_bytes(), ENVELOPE_AAD_V3_PASSWORD)?;
            let mut prefix = Vec::with_capacity(V3_PASSWORD_HEADER_LEN);
            prefix.extend_from_slice(MAGIC);
            prefix.extend_from_slice(&FORMAT_VERSION_V3.to_be_bytes());
            prefix.push(MODE_PASSWORD);
            prefix.extend_from_slice(&KDF_ALG_ARGON2ID.to_be_bytes());
            prefix.extend_from_slice(&ARGON2_DEFAULT_MEMORY_KIB.to_be_bytes());
            prefix.extend_from_slice(&ARGON2_DEFAULT_TIME_COST.to_be_bytes());
            prefix.extend_from_slice(&ARGON2_DEFAULT_PARALLELISM.to_be_bytes());
            prefix.extend_from_slice(&salt);
            prefix.extend_from_slice(&nonce);
            append_payload(&prefix, &sealed)
        }
    }
}

/// 组装明文信封：凭据全部转写到迁移密钥后序列化为 JSON。
fn assemble_envelope(
    config: &AppConfig,
    source_vault: &Vault,
    transfer_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
    usage_comparison_series: Option<&[UsageComparisonSeries]>,
) -> Result<Zeroizing<String>, ConfigTransferError> {
    let mut transferable = config.clone();
    rewrap_credentials(&mut transferable, source_vault, transfer_vault)?;
    let envelope = ExportEnvelope {
        config: transferable,
        history: history.map(|rows| rows.to_vec()),
        usage_comparison_series: usage_comparison_series
            .map(|rows| sanitize_usage_comparison_series(rows.to_vec())),
    };
    let serialized =
        Zeroizing::new(serde_json::to_string(&envelope).map_err(ConfigTransferError::Serialize)?);
    Ok(serialized)
}

/// 拼装容器字节：`prefix || u32 载荷长度 || 载荷`，超 16 MiB 上限即拒绝。
fn append_payload(prefix: &[u8], payload: &[u8]) -> Result<Vec<u8>, ConfigTransferError> {
    let total_len = prefix
        .len()
        .checked_add(payload.len())
        .and_then(|len| len.checked_add(4))
        .ok_or(ConfigTransferError::TooLarge)?;
    if total_len > MAX_EXPORT_SIZE {
        return Err(ConfigTransferError::TooLarge);
    }
    let payload_len = u32::try_from(payload.len()).map_err(|_| ConfigTransferError::TooLarge)?;
    let mut bytes = Vec::with_capacity(total_len);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

/// KDF 参数是否全部落在可接受范围内（单项上限防 KDF DoS；导出默认参数
/// 远低于上限）。抽为纯函数便于三个维度各自超限/压线的边界直测。
fn kdf_params_within_limits(memory_kib: u32, time_cost: u32, parallelism: u32) -> bool {
    (1..=KDF_MAX_MEMORY_KIB).contains(&memory_kib)
        && (1..=KDF_MAX_TIME_COST).contains(&time_cost)
        && (1..=KDF_MAX_PARALLELISM).contains(&parallelism)
}

/// 用头部自描述参数从口令派生 32 字节迁移密钥（Argon2id）。
///
/// 算法与参数范围校验先于派生执行：恶意容器头部在此快速失败，不会进入
/// 高成本的 KDF 计算。
fn derive_transfer_key(
    password: &str,
    algorithm: u32,
    memory_kib: u32,
    time_cost: u32,
    parallelism: u32,
    salt: &[u8; KDF_SALT_LEN],
) -> Result<Zeroizing<[u8; TRANSFER_KEY_LEN]>, ConfigTransferError> {
    if algorithm != KDF_ALG_ARGON2ID {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "未知的密钥派生算法",
        });
    }
    if !kdf_params_within_limits(memory_kib, time_cost, parallelism) {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "密钥派生参数超出可接受范围",
        });
    }
    let params = argon2::Params::new(memory_kib, time_cost, parallelism, Some(TRANSFER_KEY_LEN))
        .map_err(ConfigTransferError::Kdf)?;
    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = Zeroizing::new([0_u8; TRANSFER_KEY_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(ConfigTransferError::Kdf)?;
    Ok(key)
}

/// 解码迁移容器，将其中所有凭据转写到 `target_vault`，并返回可选的历史行。
///
/// 保留既有签名：对便捷档容器（v1/v2/v3 mode=0）零参数可用；密码档容器
/// 请改用 [`import_config_with_options`] 提供备份口令。
///
/// 函数只在完整认证、解析和转写成功后返回结果，不产生部分导入结果；
/// 历史行的落库合并由调用方决定（`HistoryStore::merge_rows`）。
pub fn import_config(
    bytes: &[u8],
    target_vault: &Vault,
) -> Result<TransferBundle, ConfigTransferError> {
    // 旧入口维持既有「整体替换」语义：显式传 Overwrite，不受
    // Default(Merge) 影响。本函数纯解码不落盘，strategy 仅在写入层生效。
    import_config_with_options(
        bytes,
        target_vault,
        &ImportOptions {
            password: None,
            strategy: ImportStrategy::Overwrite,
        },
    )
}

/// 解码迁移容器（可携带密码档口令），将所有凭据转写到 `target_vault`。
///
/// 密码档容器要求 `options.password`：缺失报
/// [`ConfigTransferError::PasswordRequired`]；口令错误或载荷被篡改均报
/// [`ConfigTransferError::PasswordAuthFailed`]（GCM 认证失败无法区分两者，
/// 文案如实）。便捷档容器忽略口令字段。
pub fn import_config_with_options(
    bytes: &[u8],
    target_vault: &Vault,
    options: &ImportOptions,
) -> Result<TransferBundle, ConfigTransferError> {
    let header = parse_header(bytes)?;
    let payload = &bytes[header.payload_offset..];
    let (transfer_vault, serialized) = match header.mode {
        TransferMode::Convenient => {
            let transfer_key = header
                .transfer_key
                .expect("convenient containers carry an in-package transfer key");
            let vault = Vault::from_master_key(transfer_key)?;
            let payload_str =
                std::str::from_utf8(payload).map_err(|_| ConfigTransferError::InvalidFormat {
                    reason: "加密载荷不是合法 UTF-8",
                })?;
            let plain = vault.decrypt(payload_str, header.aad)?;
            (vault, Zeroizing::new(plain.into_bytes()))
        }
        TransferMode::Password => {
            let params = header
                .password_params
                .as_ref()
                .expect("password containers carry KDF parameters");
            let password = options
                .password
                .as_deref()
                .ok_or(ConfigTransferError::PasswordRequired)?;
            let derived = derive_transfer_key(
                password,
                params.algorithm,
                params.memory_kib,
                params.time_cost,
                params.parallelism,
                &params.salt,
            )?;
            let vault = Vault::from_master_key(&derived[..])?;
            let plain = vault
                .open_with_nonce(payload, header.aad, &params.nonce)
                .map_err(|_| ConfigTransferError::PasswordAuthFailed)?;
            (vault, plain)
        }
    };
    decode_envelope(header.version, &transfer_vault, &serialized, target_vault)
}

/// 解析明文信封并把全部凭据转写到目标保险库。
/// v1 明文是裸 AppConfig，v2 起是带可选历史/比较组合的信封。
fn decode_envelope(
    version: u16,
    transfer_vault: &Vault,
    serialized: &[u8],
    target_vault: &Vault,
) -> Result<TransferBundle, ConfigTransferError> {
    if version == FORMAT_VERSION_V1 {
        let mut config: AppConfig =
            serde_json::from_slice(serialized).map_err(ConfigTransferError::Parse)?;
        rewrap_credentials(&mut config, transfer_vault, target_vault)?;
        return Ok(TransferBundle {
            config,
            history: None,
            usage_comparison_series: None,
            counts: ImportCounts::default(),
        });
    }
    let mut envelope: ExportEnvelope =
        serde_json::from_slice(serialized).map_err(ConfigTransferError::Parse)?;
    rewrap_credentials(&mut envelope.config, transfer_vault, target_vault)?;
    Ok(TransferBundle {
        config: envelope.config,
        history: envelope.history,
        usage_comparison_series: envelope
            .usage_comparison_series
            .map(sanitize_usage_comparison_series),
        counts: ImportCounts::default(),
    })
}

/// 只读识别迁移容器头部，返回容器版本与档位。
///
/// 不解密、不验证密码：密码档容器即使载荷已损坏也能识别，供 GUI 导入
/// 模态的文件信息卡在用户输入口令前展示。结构非法或版本未知时按导入
/// 同口径报错。
pub fn inspect_transfer_container(
    bytes: &[u8],
) -> Result<TransferContainerInfo, ConfigTransferError> {
    let header = parse_header(bytes)?;
    Ok(TransferContainerInfo {
        version: header.version,
        mode: header.mode,
    })
}

/// 读取前按文件元数据预拒超限迁移包（与容器解析同口径的 16 MiB 上限），
/// 避免误选的大文件先整读进内存后才报 [`ConfigTransferError::TooLarge`]；
/// 元数据读取失败按导入同口径透出 IO 错误。
pub fn precheck_transfer_file_size(path: &Path) -> Result<(), ConfigTransferError> {
    let len = std::fs::metadata(path)
        .map_err(ConfigTransferError::Read)?
        .len();
    if len > MAX_EXPORT_SIZE as u64 {
        return Err(ConfigTransferError::TooLarge);
    }
    Ok(())
}

/// 原子写出迁移包（可选携带历史，便捷档默认）；失败时清理同目录临时文件。
pub fn export_config_to_path(
    config: &AppConfig,
    source_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
    export_path: &Path,
) -> Result<(), ConfigTransferError> {
    export_config_to_path_with_usage(config, source_vault, history, None, export_path)
}

/// 原子写出携带可选统计比较组合的迁移包（便捷档默认）。
pub fn export_config_to_path_with_usage(
    config: &AppConfig,
    source_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
    usage_comparison_series: Option<&[UsageComparisonSeries]>,
    export_path: &Path,
) -> Result<(), ConfigTransferError> {
    export_config_to_path_with_options(
        config,
        source_vault,
        history,
        usage_comparison_series,
        &ExportOptions::Convenient,
        export_path,
    )
}

/// 原子写出按指定档位编码的迁移包；密码档口令不足 8 个字符时确定性拒绝。
pub fn export_config_to_path_with_options(
    config: &AppConfig,
    source_vault: &Vault,
    history: Option<&[HistoryExportRow]>,
    usage_comparison_series: Option<&[UsageComparisonSeries]>,
    options: &ExportOptions,
    export_path: &Path,
) -> Result<(), ConfigTransferError> {
    let bytes = export_config_with_options(
        config,
        source_vault,
        history,
        usage_comparison_series,
        options,
    )?;
    atomic_write(export_path, &bytes).map_err(ConfigTransferError::Write)
}

/// 从文件读取迁移包并返回已转写到目标保险库的配置与可选历史
/// （便捷档默认；密码档包请改用 [`import_config_from_path_with_options`]）。
///
/// 纯解码入口不落盘；旧入口维持「整体替换」语义（显式传 Overwrite，
/// strategy 仅在写入层生效）。
pub fn import_config_from_path(
    export_path: &Path,
    target_vault: &Vault,
) -> Result<TransferBundle, ConfigTransferError> {
    import_config_from_path_with_options(
        export_path,
        target_vault,
        &ImportOptions {
            password: None,
            strategy: ImportStrategy::Overwrite,
        },
    )
}

/// 从文件读取迁移包（可携带密码档口令）并返回已转写的配置与可选历史。
pub fn import_config_from_path_with_options(
    export_path: &Path,
    target_vault: &Vault,
    options: &ImportOptions,
) -> Result<TransferBundle, ConfigTransferError> {
    let bytes = std::fs::read(export_path).map_err(ConfigTransferError::Read)?;
    import_config_with_options(&bytes, target_vault, options)
}

/// 完整导入迁移包后，原子替换目标配置文件并返回解码结果（含待合并历史）；
/// 便捷档默认，密码档包请改用 [`import_config_to_path_with_options`]。
///
/// 旧入口维持既有「整体替换」现状语义：显式传 [`ImportStrategy::Overwrite`]
/// 委托，不受 `Default`（合并）影响。
pub fn import_config_to_path(
    export_path: &Path,
    target_vault: &Vault,
    config_path: &Path,
) -> Result<TransferBundle, ConfigTransferError> {
    import_config_to_path_with_options(
        export_path,
        target_vault,
        &ImportOptions {
            password: None,
            strategy: ImportStrategy::Overwrite,
        },
        config_path,
    )
}

/// 完整导入迁移包（可携带密码档口令与导入策略）后应用到目标配置文件。
///
/// 策略在写入层生效：合并 = 读取本机配置做条目/自定义模型库并集（同 id
/// 本机为准）后原子保存；覆盖 = 现状整体替换。历史与比较组合不落
/// config.json：历史由调用端按策略选 `HistoryStore::merge_rows`（合并，
/// 现状幂等合并）或 `replace_rows`（覆盖，单事务清空重插）；比较组合由
/// 调用端按策略整体写入 settings.json（覆盖）或经
/// [`merge_usage_comparison_series`] 并集（合并）。生效计数见返回
/// bundle 的 `counts` 字段。
pub fn import_config_to_path_with_options(
    export_path: &Path,
    target_vault: &Vault,
    options: &ImportOptions,
    config_path: &Path,
) -> Result<TransferBundle, ConfigTransferError> {
    let bytes = std::fs::read(export_path).map_err(ConfigTransferError::Read)?;
    import_config_bytes_to_path_with_options(&bytes, target_vault, options, config_path)
}

/// 用已读取的容器字节完整导入（解码 + 策略写入层 + 生效计数），语义与
/// [`import_config_to_path_with_options`] 一致；供单次读取复用同一份字节
/// 的调用端（CLI 先 inspect 判档收口令再导入，消除对同一文件的二次读取
/// 竞态窗口）。
pub fn import_config_bytes_to_path_with_options(
    bytes: &[u8],
    target_vault: &Vault,
    options: &ImportOptions,
    config_path: &Path,
) -> Result<TransferBundle, ConfigTransferError> {
    let mut bundle = import_config_with_options(bytes, target_vault, options)?;
    bundle.counts = match options.strategy {
        ImportStrategy::Merge => {
            // 本机配置文件缺失视为空配置（首次恢复场景 → 全额并入）。
            let local = AppConfig::load(config_path).map_err(ConfigTransferError::Load)?;
            let (merged, counts) = merge_app_config(&local, &bundle.config);
            merged.save(config_path)?;
            counts
        }
        ImportStrategy::Overwrite => {
            bundle.config.save(config_path)?;
            ImportCounts {
                providers_added: bundle.config.providers.len(),
                providers_skipped: 0,
                series_added: bundle.usage_comparison_series.as_ref().map_or(0, Vec::len),
                series_skipped: 0,
            }
        }
    };
    Ok(bundle)
}

/// 合并模的配置并集：条目按 id 并集（本机在前保序、备份新条目按序追加；
/// 同 id 冲突以本机为准、跳过备份条目无需凭据转写——本机密文 AAD 未动，
/// 新条目在解码层已转写到目标 vault）；自定义模型库按 native 键并集、
/// 同键同模型 id 以本机定义为准（「不丢本机任何东西」覆盖 custom_models
/// 维度，条目计数仅覆盖 providers）。
fn merge_app_config(local: &AppConfig, incoming: &AppConfig) -> (AppConfig, ImportCounts) {
    let mut merged = local.clone();
    let mut ids: HashSet<&str> = local
        .providers
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let mut counts = ImportCounts::default();
    for entry in &incoming.providers {
        if ids.insert(entry.id.as_str()) {
            merged.providers.push(entry.clone());
            counts.providers_added += 1;
        } else {
            counts.providers_skipped += 1;
        }
    }
    for (key, models) in &incoming.custom_models {
        let slot = merged.custom_models.entry(key.clone()).or_default();
        for model in models {
            if !slot.iter().any(|existing| existing.id == model.id) {
                slot.push(model.clone());
            }
        }
    }
    (merged, counts)
}

fn rewrap_credentials(
    config: &mut AppConfig,
    source_vault: &Vault,
    target_vault: &Vault,
) -> Result<(), ConfigTransferError> {
    for provider in &mut config.providers {
        let Some(ciphertext) = provider.api_key_enc.as_deref() else {
            continue;
        };
        let plaintext = Zeroizing::new(source_vault.decrypt(ciphertext, &provider.id)?);
        provider.api_key_enc = Some(target_vault.encrypt(&plaintext, &provider.id)?);
        // 第二凭据槽同规则转写（未配置则跳过）
        if let Some(ciphertext2) = provider.api_key2_enc.as_deref() {
            let plaintext2 = Zeroizing::new(source_vault.decrypt(ciphertext2, &provider.id)?);
            provider.api_key2_enc = Some(target_vault.encrypt(&plaintext2, &provider.id)?);
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temporary_path(path);
    let result = (|| {
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("export");
    path.with_extension(format!("{extension}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use crate::config::{PlanVariant, ProviderEntry, ProviderKind};
    use crate::pricing::{CustomModelDef, PriceTier, PricingConfig};
    use crate::vault::{InMemoryStore, SecretStore};

    use super::*;

    const SECRET_A: &str = "sk-source-secret-a";
    const SECRET_B: &str = "sk-source-secret-b";

    fn temp_path(tag: &str, extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "quotatray-transfer-{tag}-{}-{extension}",
            std::process::id()
        ))
    }

    fn sample_config(vault: &Vault) -> AppConfig {
        let mut native = ProviderEntry {
            id: "native-a".into(),
            name: "敏感显示名称".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: Some("https://sensitive.example.test".into()),
            pricing: Some(PricingConfig {
                model: Some("private-model".into()),
                peak: Some(PriceTier::full(0.1, 1.0, 2.0)),
                ..Default::default()
            }),
            plan_variant: PlanVariant::Weekly,
            use_proxy: false,
            console_url: None,
        };
        native.set_api_key(vault, SECRET_A).unwrap();

        let template = serde_json::from_value(serde_json::json!({
            "request": {
                "url": "{{baseUrl}}/balance",
                "headers": { "Authorization": "Bearer {{apiKey}}" }
            },
            "extract": {
                "remaining": "$.balance",
                "unit": { "const": "USD" }
            }
        }))
        .unwrap();
        let mut custom = ProviderEntry {
            id: "template-b".into(),
            name: "Template B".into(),
            kind: ProviderKind::Template(Box::new(template)),
            enabled: false,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: Some("https://template.example.test".into()),
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
            console_url: None,
        };
        custom.set_api_key(vault, SECRET_B).unwrap();

        let mut custom_models = BTreeMap::new();
        custom_models.insert(
            "deepseek".into(),
            vec![CustomModelDef {
                id: "private-model".into(),
                display: "私有计价模型".into(),
                peak: Some(PriceTier::full(0.2, 2.0, 4.0)),
                ..Default::default()
            }],
        );
        AppConfig {
            providers: vec![native, custom],
            custom_models,
        }
    }

    #[test]
    fn cross_machine_roundtrip_rewraps_every_credential() {
        let source_store = InMemoryStore::new();
        let target_store = InMemoryStore::new();
        let source_vault = Vault::open(&source_store).unwrap();
        let target_vault = Vault::open(&target_store).unwrap();
        let source_key_before = source_store.get().unwrap().unwrap();
        let target_key_before = target_store.get().unwrap().unwrap();
        let config = sample_config(&source_vault);

        let bytes = export_config(&config, &source_vault, None).unwrap();
        let imported = import_config(&bytes, &target_vault).unwrap();

        assert_eq!(imported.config.providers.len(), 2);
        assert_eq!(imported.config.custom_models, config.custom_models);
        assert!(
            imported.history.is_none(),
            "未携带历史时 bundle.history 为 None"
        );
        assert!(
            imported.usage_comparison_series.is_none(),
            "旧 v2 形态未携带比较组合时保持 None"
        );
        assert_eq!(
            imported.config.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A
        );
        assert_eq!(
            imported.config.providers[1]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_B
        );
        assert_ne!(
            imported.config.providers[0].api_key_enc,
            config.providers[0].api_key_enc
        );
        assert_eq!(source_store.get().unwrap().unwrap(), source_key_before);
        assert_eq!(target_store.get().unwrap().unwrap(), target_key_before);
        assert!(!bytes.windows(32).any(|window| window == source_key_before));
        assert!(!bytes.windows(32).any(|window| window == target_key_before));
    }

    #[test]
    fn export_is_opaque_and_uses_a_fresh_transfer_key() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config = sample_config(&source_vault);
        let history = sample_history();
        let first = export_config(&config, &source_vault, Some(&history)).unwrap();
        let second = export_config(&config, &source_vault, Some(&history)).unwrap();

        assert_ne!(first, second);
        assert_ne!(
            &first[V3_CONVENIENT_KEY_OFFSET..V3_CONVENIENT_LENGTH_OFFSET],
            &second[V3_CONVENIENT_KEY_OFFSET..V3_CONVENIENT_LENGTH_OFFSET],
            "每次导出必须换迁移密钥"
        );
        for plain in [
            SECRET_A,
            SECRET_B,
            "敏感显示名称",
            "sensitive.example.test",
            "私有计价模型",
            "history-provider-a",
        ] {
            assert!(
                !first
                    .windows(plain.len())
                    .any(|window| window == plain.as_bytes()),
                "导出包泄漏明文：{plain}"
            );
        }
        assert!(serde_json::from_slice::<serde_json::Value>(&first).is_err());
    }

    #[test]
    fn empty_missing_and_empty_string_credentials_roundtrip() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let empty = export_config(&AppConfig::default(), &source_vault, None).unwrap();
        assert_eq!(
            import_config(&empty, &target_vault).unwrap().config,
            AppConfig::default()
        );

        let mut config = sample_config(&source_vault);
        config.providers[0].api_key_enc = None;
        config.providers[1].set_api_key(&source_vault, "").unwrap();
        let imported = import_config(
            &export_config(&config, &source_vault, None).unwrap(),
            &target_vault,
        )
        .unwrap();
        assert!(imported.config.providers[0].api_key_enc.is_none());
        assert_eq!(
            imported.config.providers[1]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            ""
        );
    }

    #[test]
    fn malformed_or_tampered_packages_are_rejected() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let valid = export_config(&sample_config(&source_vault), &source_vault, None).unwrap();

        let mut bad_magic = valid.clone();
        bad_magic[0] ^= 1;
        assert!(import_config(&bad_magic, &target_vault).is_err());

        let mut bad_version = valid.clone();
        // v3 起合法最高版本为 3，篡改目标前移到未来版本 4（改 3 已是恒等操作）。
        bad_version[VERSION_OFFSET..KEY_OFFSET].copy_from_slice(&4_u16.to_be_bytes());
        assert!(import_config(&bad_version, &target_vault).is_err());

        assert!(import_config(&valid[..20], &target_vault).is_err());

        let mut bad_length = valid.clone();
        bad_length[V3_CONVENIENT_LENGTH_OFFSET..V3_CONVENIENT_HEADER_LEN]
            .copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(import_config(&bad_length, &target_vault).is_err());

        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(import_config(&trailing, &target_vault).is_err());

        let mut tampered = valid;
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(import_config(&tampered, &target_vault).is_err());

        let oversized = vec![0_u8; MAX_EXPORT_SIZE + 1];
        assert!(matches!(
            import_config(&oversized, &target_vault),
            Err(ConfigTransferError::TooLarge)
        ));
    }

    #[test]
    fn authenticated_but_invalid_content_is_rejected() {
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let (transfer_vault, transfer_key) = Vault::transient().unwrap();

        let invalid_json = package_v1_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt("{not valid json", ENVELOPE_AAD_V1)
                .unwrap(),
        );
        assert!(matches!(
            import_config(&invalid_json, &target_vault),
            Err(ConfigTransferError::Parse(_))
        ));

        let wrong_envelope_aad = package_v1_with_payload(
            &transfer_key,
            &transfer_vault.encrypt("{}", "wrong-envelope-aad").unwrap(),
        );
        assert!(matches!(
            import_config(&wrong_envelope_aad, &target_vault),
            Err(ConfigTransferError::Vault(_))
        ));

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut config = sample_config(&source_vault);
        rewrap_credentials(&mut config, &source_vault, &transfer_vault).unwrap();
        config.providers[0].id = "changed-after-encryption".into();
        let serialized = serde_json::to_string(&config).unwrap();
        let bad_credential_aad = package_v1_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serialized, ENVELOPE_AAD_V1)
                .unwrap(),
        );
        assert!(matches!(
            import_config(&bad_credential_aad, &target_vault),
            Err(ConfigTransferError::Vault(_))
        ));
    }

    #[test]
    fn corrupt_source_credential_aborts_export() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut config = sample_config(&source_vault);
        config.providers[0].api_key_enc = Some("v1:AAAA".into());
        assert!(export_config(&config, &source_vault, None).is_err());
    }

    #[test]
    fn path_helpers_replace_only_after_successful_import() {
        let export_path = temp_path("bundle", CONFIG_EXPORT_EXTENSION);
        let config_path = temp_path("target", "json");
        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let source = sample_config(&source_vault);
        let original = AppConfig::default();
        original.save(&config_path).unwrap();

        fs::write(&export_path, b"not a QuotaTray package").unwrap();
        assert!(import_config_to_path(&export_path, &target_vault, &config_path).is_err());
        assert_eq!(AppConfig::load(&config_path).unwrap(), original);

        export_config_to_path(&source, &source_vault, None, &export_path).unwrap();
        let decoded = import_config_from_path(&export_path, &target_vault).unwrap();
        let saved = import_config_to_path(&export_path, &target_vault, &config_path).unwrap();
        assert_eq!(decoded.config.custom_models, saved.config.custom_models);
        assert_eq!(decoded.config.providers.len(), saved.config.providers.len());
        for (decoded_entry, saved_entry) in
            decoded.config.providers.iter().zip(&saved.config.providers)
        {
            let mut decoded_public = decoded_entry.clone();
            let mut saved_public = saved_entry.clone();
            decoded_public.api_key_enc = None;
            saved_public.api_key_enc = None;
            assert_eq!(decoded_public, saved_public);
            assert_eq!(
                decoded_entry
                    .credentials(&target_vault)
                    .unwrap()
                    .api_key
                    .as_str(),
                saved_entry
                    .credentials(&target_vault)
                    .unwrap()
                    .api_key
                    .as_str()
            );
        }
        assert_eq!(AppConfig::load(&config_path).unwrap(), saved.config);
        assert_eq!(
            saved.config.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A
        );

        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);
    }

    /// 手工构造 v1 容器（裸 AppConfig 载荷），用于验证对历史版本的兼容与拒绝。
    fn package_v1_with_payload(transfer_key: &[u8], payload: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION_V1.to_be_bytes());
        bytes.extend_from_slice(transfer_key);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload.as_bytes());
        bytes
    }

    fn sample_history() -> Vec<HistoryExportRow> {
        vec![
            HistoryExportRow {
                provider_id: "history-provider-a".into(),
                window_key: "five_hour".into(),
                sampled_at: 1_700_000_000_000,
                used: Some(12.0),
                remaining: Some(88.0),
                total: Some(100.0),
                unit: Some("USD".into()),
            },
            HistoryExportRow {
                provider_id: "history-provider-a".into(),
                window_key: "weekly".into(),
                sampled_at: 1_700_000_000_000,
                used: Some(40.0),
                remaining: Some(60.0),
                total: Some(100.0),
                unit: Some("%".into()),
            },
        ]
    }

    #[test]
    fn history_roundtrips_inside_v2_bundle() {
        let source_store = InMemoryStore::new();
        let target_store = InMemoryStore::new();
        let source_vault = Vault::open(&source_store).unwrap();
        let target_vault = Vault::open(&target_store).unwrap();
        let config = sample_config(&source_vault);
        let history = sample_history();

        let bytes = export_config(&config, &source_vault, Some(&history)).unwrap();
        let bundle = import_config(&bytes, &target_vault).unwrap();

        assert_eq!(bundle.history, Some(history));
        // 携带历史不得破坏凭据转写。
        assert_eq!(
            bundle.config.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A
        );
    }

    #[test]
    fn usage_comparison_roundtrips_as_optional_v2_metadata() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config = sample_config(&source_vault);
        let comparison = vec![UsageComparisonSeries {
            provider_id: "native-a".into(),
            window_key: "Codex（5h）".into(),
            color_slot: 2,
        }];

        let bytes =
            export_config_with_usage(&config, &source_vault, None, Some(&comparison)).unwrap();
        let bundle = import_config(&bytes, &target_vault).unwrap();

        assert_eq!(bundle.usage_comparison_series, Some(comparison));
    }

    #[test]
    fn usage_comparison_sanitize_trims_deduplicates_caps_and_repairs_slots() {
        let items = vec![
            UsageComparisonSeries {
                provider_id: " p1 ".into(),
                window_key: " w1 ".into(),
                color_slot: 3,
            },
            UsageComparisonSeries {
                provider_id: "p1".into(),
                window_key: "w1".into(),
                color_slot: 0,
            },
            UsageComparisonSeries {
                provider_id: "p2".into(),
                window_key: "w2".into(),
                color_slot: 3,
            },
            UsageComparisonSeries {
                provider_id: "p3".into(),
                window_key: "w3".into(),
                color_slot: 9,
            },
            UsageComparisonSeries {
                provider_id: "p4".into(),
                window_key: "w4".into(),
                color_slot: 1,
            },
            UsageComparisonSeries {
                provider_id: "p5".into(),
                window_key: "w5".into(),
                color_slot: 2,
            },
        ];

        let sanitized = sanitize_usage_comparison_series(items);
        assert_eq!(sanitized.len(), 4);
        assert_eq!(sanitized[0].provider_id, "p1");
        assert_eq!(sanitized[0].window_key, "w1");
        assert_eq!(
            sanitized
                .iter()
                .map(|item| item.color_slot)
                .collect::<Vec<_>>(),
            vec![3, 0, 1, 2]
        );
    }

    #[test]
    fn v1_package_without_history_imports_as_none() {
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let (transfer_vault, transfer_key) = Vault::transient().unwrap();
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();

        let mut config = sample_config(&source_vault);
        rewrap_credentials(&mut config, &source_vault, &transfer_vault).unwrap();
        let serialized = serde_json::to_string(&config).unwrap();
        let v1_bytes = package_v1_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serialized, ENVELOPE_AAD_V1)
                .unwrap(),
        );

        let bundle = import_config(&v1_bytes, &target_vault).unwrap();
        assert_eq!(bundle.config.providers.len(), 2);
        assert!(bundle.history.is_none());
        assert!(bundle.usage_comparison_series.is_none());
    }

    #[test]
    fn oversized_history_export_fails() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config = sample_config(&source_vault);
        // 单行约 1 MiB 的 unit 字符串，20 行必然超过 16 MiB 上限。
        let fat_unit = "x".repeat(1024 * 1024);
        let rows: Vec<HistoryExportRow> = (0..20)
            .map(|idx| HistoryExportRow {
                provider_id: format!("p-{idx}"),
                window_key: "w0".into(),
                sampled_at: idx,
                used: None,
                remaining: None,
                total: None,
                unit: Some(fat_unit.clone()),
            })
            .collect();
        assert!(matches!(
            export_config(&config, &source_vault, Some(&rows)),
            Err(ConfigTransferError::TooLarge)
        ));
    }

    // ---- 迁移容器 v3：密码档导出与 inspect 识别（工单 #120）----

    const V3_PASSWORD: &str = "correct-horse-battery-staple";

    /// 手工构造 v2 容器（信封载荷），锁定对 v2 布局与 AAD 的兼容。
    fn package_v2_with_payload(transfer_key: &[u8], payload: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION_V2.to_be_bytes());
        bytes.extend_from_slice(transfer_key);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload.as_bytes());
        bytes
    }

    /// 手工构造 v3 便捷档容器（mode 字节 + 随包密钥 + v3 便捷 AAD 载荷）。
    fn package_v3_convenient_with_payload(transfer_key: &[u8], payload: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(V3_CONVENIENT_HEADER_LEN + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION_V3.to_be_bytes());
        bytes.push(MODE_CONVENIENT);
        bytes.extend_from_slice(transfer_key);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload.as_bytes());
        bytes
    }

    /// 手工构造 v3 密码档容器（自描述 KDF 头部 + nonce + 任意载荷），
    /// 供 AAD/参数校验与 inspect 契约测试。
    #[allow(clippy::too_many_arguments)]
    fn package_v3_password_with_payload(
        algorithm: u32,
        memory_kib: u32,
        time_cost: u32,
        parallelism: u32,
        salt: &[u8; KDF_SALT_LEN],
        nonce: &[u8; CIPHER_NONCE_LEN],
        payload: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(V3_PASSWORD_HEADER_LEN + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION_V3.to_be_bytes());
        bytes.push(MODE_PASSWORD);
        bytes.extend_from_slice(&algorithm.to_be_bytes());
        bytes.extend_from_slice(&memory_kib.to_be_bytes());
        bytes.extend_from_slice(&time_cost.to_be_bytes());
        bytes.extend_from_slice(&parallelism.to_be_bytes());
        bytes.extend_from_slice(salt);
        bytes.extend_from_slice(nonce);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn password_export_import_roundtrip_hides_derived_key() {
        let source_store = InMemoryStore::new();
        let target_store = InMemoryStore::new();
        let source_vault = Vault::open(&source_store).unwrap();
        let target_vault = Vault::open(&target_store).unwrap();
        let source_key_before = source_store.get().unwrap().unwrap();
        let target_key_before = target_store.get().unwrap().unwrap();
        let mut config = sample_config(&source_vault);
        // 双凭据槽：第二槽必须在同一次导出中一并转写。
        config.providers[0]
            .set_api_key2(&source_vault, "user-id-1024")
            .unwrap();
        let history = sample_history();
        let comparison = vec![UsageComparisonSeries {
            provider_id: "native-a".into(),
            window_key: "five_hour".into(),
            color_slot: 1,
        }];

        let bytes = export_config_with_options(
            &config,
            &source_vault,
            Some(&history),
            Some(&comparison),
            &ExportOptions::Password {
                password: V3_PASSWORD.into(),
            },
        )
        .unwrap();

        // 头部自描述：不解密即可识别为 v3 密码档。
        let info = inspect_transfer_container(&bytes).unwrap();
        assert_eq!(info.version, FORMAT_VERSION_V3);
        assert_eq!(info.mode, TransferMode::Password);

        // 攻击者视角：知道口令、能读头部全部 KDF 参数，独立派生出的密钥
        // 也不得出现在容器任何字节。
        let salt: [u8; KDF_SALT_LEN] = bytes[KDF_SALT_OFFSET..CIPHER_NONCE_OFFSET]
            .try_into()
            .unwrap();
        let memory_kib = u32::from_be_bytes(
            bytes[KDF_MEMORY_OFFSET..KDF_TIME_OFFSET]
                .try_into()
                .unwrap(),
        );
        let time_cost = u32::from_be_bytes(
            bytes[KDF_TIME_OFFSET..KDF_PARALLELISM_OFFSET]
                .try_into()
                .unwrap(),
        );
        let parallelism = u32::from_be_bytes(
            bytes[KDF_PARALLELISM_OFFSET..KDF_SALT_OFFSET]
                .try_into()
                .unwrap(),
        );
        let params = argon2::Params::new(memory_kib, time_cost, parallelism, Some(32)).unwrap();
        let mut derived = [0u8; 32];
        argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(V3_PASSWORD.as_bytes(), &salt, &mut derived)
            .unwrap();
        assert!(
            !bytes.windows(32).any(|window| window == derived),
            "派生密钥不得出现在容器任何字节"
        );
        assert!(!bytes.windows(32).any(|window| window == source_key_before));
        assert!(!bytes.windows(32).any(|window| window == target_key_before));
        for plain in [
            V3_PASSWORD,
            SECRET_A,
            SECRET_B,
            "user-id-1024",
            "敏感显示名称",
        ] {
            assert!(
                !bytes
                    .windows(plain.len())
                    .any(|window| window == plain.as_bytes()),
                "密码档容器泄漏明文：{plain}"
            );
        }

        let bundle = import_config_with_options(
            &bytes,
            &target_vault,
            &ImportOptions {
                password: Some(V3_PASSWORD.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(bundle.config.providers.len(), 2);
        assert_eq!(bundle.history, Some(history));
        assert_eq!(bundle.usage_comparison_series, Some(comparison));
        let credentials = bundle.config.providers[0]
            .credentials(&target_vault)
            .unwrap();
        assert_eq!(credentials.api_key.as_str(), SECRET_A);
        assert_eq!(
            credentials.api_key2.as_ref().unwrap().as_str(),
            "user-id-1024"
        );
        assert_eq!(
            bundle.config.providers[1]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_B
        );
        assert_eq!(source_store.get().unwrap().unwrap(), source_key_before);
        assert_eq!(target_store.get().unwrap().unwrap(), target_key_before);
    }

    #[test]
    fn password_export_rejects_passwords_shorter_than_8_chars() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config = sample_config(&source_vault);

        let err = export_config_with_options(
            &config,
            &source_vault,
            None,
            None,
            &ExportOptions::Password {
                password: "1234567".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, ConfigTransferError::PasswordTooShort));
        assert!(err.to_string().contains("8"), "文案应说明最小长度：{err}");

        // 恰好 8 个字符：长度维度不再拒绝，可完整往返。
        let bytes = export_config_with_options(
            &config,
            &source_vault,
            None,
            None,
            &ExportOptions::Password {
                password: "12345678".into(),
            },
        )
        .unwrap();
        let bundle = import_config_with_options(
            &bytes,
            &Vault::open(&InMemoryStore::new()).unwrap(),
            &ImportOptions {
                password: Some("12345678".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(bundle.config.providers.len(), 2);
    }

    #[test]
    fn wrong_or_missing_password_fails_deterministically() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let bytes = export_config_with_options(
            &sample_config(&source_vault),
            &source_vault,
            None,
            None,
            &ExportOptions::Password {
                password: V3_PASSWORD.into(),
            },
        )
        .unwrap();

        // 未提供密码：确定性引导错误，而非笼统的解析失败。
        let missing = import_config_with_options(&bytes, &target_vault, &ImportOptions::default())
            .unwrap_err();
        assert!(matches!(missing, ConfigTransferError::PasswordRequired));

        // 错误密码：GCM 认证失败无法区分密码错误与包损坏，文案如实。
        let wrong = import_config_with_options(
            &bytes,
            &target_vault,
            &ImportOptions {
                password: Some("wrong-password-42".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(wrong, ConfigTransferError::PasswordAuthFailed));
        assert_eq!(wrong.to_string(), "备份密码错误或包已损坏");

        // 正确密码 + 篡改密文：同一确定性文案（包损坏方向）。
        let mut tampered = bytes.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        let corrupted = import_config_with_options(
            &tampered,
            &target_vault,
            &ImportOptions {
                password: Some(V3_PASSWORD.into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(matches!(corrupted, ConfigTransferError::PasswordAuthFailed));
    }

    #[test]
    fn inspect_reports_version_and_mode_without_decryption() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let (transfer_vault, transfer_key) = Vault::transient().unwrap();

        // v1 手工包（裸 AppConfig 载荷）。
        let mut v1_config = sample_config(&source_vault);
        rewrap_credentials(&mut v1_config, &source_vault, &transfer_vault).unwrap();
        let v1 = package_v1_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serde_json::to_string(&v1_config).unwrap(), ENVELOPE_AAD_V1)
                .unwrap(),
        );
        // v2 手工包（信封载荷）。
        let mut v2_config = sample_config(&source_vault);
        rewrap_credentials(&mut v2_config, &source_vault, &transfer_vault).unwrap();
        let envelope = ExportEnvelope {
            config: v2_config,
            history: None,
            usage_comparison_series: None,
        };
        let v2 = package_v2_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serde_json::to_string(&envelope).unwrap(), ENVELOPE_AAD_V2)
                .unwrap(),
        );

        let v1_info = inspect_transfer_container(&v1).unwrap();
        assert_eq!(
            (v1_info.version, v1_info.mode),
            (FORMAT_VERSION_V1, TransferMode::Convenient)
        );
        let v2_info = inspect_transfer_container(&v2).unwrap();
        assert_eq!(
            (v2_info.version, v2_info.mode),
            (FORMAT_VERSION_V2, TransferMode::Convenient)
        );

        let convenient = export_config_with_options(
            &sample_config(&source_vault),
            &source_vault,
            None,
            None,
            &ExportOptions::Convenient,
        )
        .unwrap();
        assert_eq!(
            inspect_transfer_container(&convenient).unwrap(),
            TransferContainerInfo {
                version: FORMAT_VERSION_V3,
                mode: TransferMode::Convenient,
            }
        );

        let password = export_config_with_options(
            &sample_config(&source_vault),
            &source_vault,
            None,
            None,
            &ExportOptions::Password {
                password: V3_PASSWORD.into(),
            },
        )
        .unwrap();
        assert_eq!(
            inspect_transfer_container(&password).unwrap(),
            TransferContainerInfo {
                version: FORMAT_VERSION_V3,
                mode: TransferMode::Password,
            }
        );

        // 密码档载荷损坏不影响 inspect：只读头部，不触碰密码学验证。
        let mut corrupted = password;
        let last = corrupted.len() - 1;
        corrupted[last] ^= 1;
        assert_eq!(
            inspect_transfer_container(&corrupted).unwrap().mode,
            TransferMode::Password
        );

        // 坏输入：空、截断、未知版本。
        assert!(inspect_transfer_container(b"").is_err());
        assert!(inspect_transfer_container(&v1[..20]).is_err());
        let mut v4 = convenient;
        v4[VERSION_OFFSET..KEY_OFFSET].copy_from_slice(&4_u16.to_be_bytes());
        assert!(matches!(
            inspect_transfer_container(&v4),
            Err(ConfigTransferError::UnsupportedVersion { version: 4 })
        ));
    }

    #[test]
    fn v1_v2_v3_packages_import_through_one_entry_point() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let (transfer_vault, transfer_key) = Vault::transient().unwrap();
        let history = sample_history();
        let comparison = vec![UsageComparisonSeries {
            provider_id: "native-a".into(),
            window_key: "five_hour".into(),
            color_slot: 0,
        }];

        // v1：裸 AppConfig 载荷。
        let mut v1_config = sample_config(&source_vault);
        rewrap_credentials(&mut v1_config, &source_vault, &transfer_vault).unwrap();
        let v1 = package_v1_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serde_json::to_string(&v1_config).unwrap(), ENVELOPE_AAD_V1)
                .unwrap(),
        );

        // v2：信封载荷（携带历史与比较组合）。
        let mut v2_config = sample_config(&source_vault);
        rewrap_credentials(&mut v2_config, &source_vault, &transfer_vault).unwrap();
        let envelope = ExportEnvelope {
            config: v2_config,
            history: Some(history.clone()),
            usage_comparison_series: Some(comparison.clone()),
        };
        let v2 = package_v2_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serde_json::to_string(&envelope).unwrap(), ENVELOPE_AAD_V2)
                .unwrap(),
        );

        // v3 两档：既有导出入口（便捷档默认）与密码档。
        let v3_convenient = export_config_with_usage(
            &sample_config(&source_vault),
            &source_vault,
            Some(&history),
            Some(&comparison),
        )
        .unwrap();
        let v3_password = export_config_with_options(
            &sample_config(&source_vault),
            &source_vault,
            Some(&history),
            Some(&comparison),
            &ExportOptions::Password {
                password: V3_PASSWORD.into(),
            },
        )
        .unwrap();

        // 四代包同走新导入入口；便捷系无需密码。
        let no_password = ImportOptions::default();
        let from_v1 = import_config_with_options(&v1, &target_vault, &no_password).unwrap();
        assert_eq!(from_v1.config.providers.len(), 2);
        assert!(from_v1.history.is_none());
        assert!(from_v1.usage_comparison_series.is_none());
        assert_eq!(
            from_v1.config.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A
        );

        let from_v2 = import_config_with_options(&v2, &target_vault, &no_password).unwrap();
        assert_eq!(from_v2.history, Some(history.clone()));
        assert_eq!(from_v2.usage_comparison_series, Some(comparison.clone()));

        // 既有无 options 导入入口对 v3 便捷档直接可用（CLI/GUI 零改动验证）。
        let from_v3c = import_config(&v3_convenient, &target_vault).unwrap();
        assert_eq!(from_v3c.history, Some(history.clone()));
        assert_eq!(from_v3c.usage_comparison_series, Some(comparison.clone()));
        assert_eq!(
            from_v3c.config.providers[1]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_B
        );

        let from_v3p = import_config_with_options(
            &v3_password,
            &target_vault,
            &ImportOptions {
                password: Some(V3_PASSWORD.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(from_v3p.history, Some(history));
        assert_eq!(from_v3p.usage_comparison_series, Some(comparison));
    }

    #[test]
    fn v3_convenient_aad_is_scoped_to_version_and_mode() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let (transfer_vault, transfer_key) = Vault::transient().unwrap();
        let mut config = sample_config(&source_vault);
        rewrap_credentials(&mut config, &source_vault, &transfer_vault).unwrap();
        let envelope = ExportEnvelope {
            config,
            history: None,
            usage_comparison_series: None,
        };
        let serialized = serde_json::to_string(&envelope).unwrap();

        // v3 便捷档专属 AAD：布局与密钥相同，仅 AAD 正确时可导入。
        let ok = package_v3_convenient_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serialized, ENVELOPE_AAD_V3_CONVENIENT)
                .unwrap(),
        );
        let imported = import_config(&ok, &target_vault).unwrap();
        assert_eq!(imported.config.providers.len(), 2);

        // 用 v2 的 AAD 加密同载荷：v3 便捷档头部拒绝（AAD 按版本+档位细分）。
        let wrong = package_v3_convenient_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt(&serialized, ENVELOPE_AAD_V2)
                .unwrap(),
        );
        assert!(import_config(&wrong, &target_vault).is_err());
    }

    #[test]
    fn password_export_respects_shared_size_cap() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config = sample_config(&source_vault);
        // 单行约 1 MiB 的 unit 字符串，20 行必然超过 16 MiB 上限（两档同限）。
        let fat_unit = "x".repeat(1024 * 1024);
        let rows: Vec<HistoryExportRow> = (0..20)
            .map(|idx| HistoryExportRow {
                provider_id: format!("p-{idx}"),
                window_key: "w0".into(),
                sampled_at: idx,
                used: None,
                remaining: None,
                total: None,
                unit: Some(fat_unit.clone()),
            })
            .collect();
        assert!(matches!(
            export_config_with_options(
                &config,
                &source_vault,
                Some(&rows),
                None,
                &ExportOptions::Password {
                    password: V3_PASSWORD.into(),
                },
            ),
            Err(ConfigTransferError::TooLarge)
        ));
    }

    #[test]
    fn hostile_kdf_params_are_rejected_before_derivation() {
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let salt = [0_u8; KDF_SALT_LEN];
        let nonce = [0_u8; CIPHER_NONCE_LEN];
        let with_password = || ImportOptions {
            password: Some(V3_PASSWORD.into()),
            ..Default::default()
        };

        // 未知 KDF 算法：报格式错误，而不是按未知算法派生。
        let unknown_alg = package_v3_password_with_payload(
            99,
            ARGON2_DEFAULT_MEMORY_KIB,
            ARGON2_DEFAULT_TIME_COST,
            ARGON2_DEFAULT_PARALLELISM,
            &salt,
            &nonce,
            b"payload",
        );
        assert!(matches!(
            import_config_with_options(&unknown_alg, &target_vault, &with_password()),
            Err(ConfigTransferError::InvalidFormat { .. })
        ));

        // 巨额内存参数：必须在参数校验层快速拒绝（防 KDF DoS），不得进入派生。
        let hostile_memory = package_v3_password_with_payload(
            KDF_ALG_ARGON2ID,
            u32::MAX,
            ARGON2_DEFAULT_TIME_COST,
            ARGON2_DEFAULT_PARALLELISM,
            &salt,
            &nonce,
            b"payload",
        );
        let start = std::time::Instant::now();
        assert!(matches!(
            import_config_with_options(&hostile_memory, &target_vault, &with_password()),
            Err(ConfigTransferError::InvalidFormat { .. })
        ));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "巨额 KDF 参数必须在派生前拒绝"
        );
    }

    /// KDF 参数上限的维度完整回归：三个维度各自超限都拒绝、恰好压线都
    /// 放行（压线放行只测纯校验函数，不实际执行天价派生）。
    #[test]
    fn kdf_param_limits_cover_all_dimensions() {
        assert!(!kdf_params_within_limits(
            KDF_MAX_MEMORY_KIB + 1,
            ARGON2_DEFAULT_TIME_COST,
            ARGON2_DEFAULT_PARALLELISM
        ));
        assert!(!kdf_params_within_limits(
            ARGON2_DEFAULT_MEMORY_KIB,
            KDF_MAX_TIME_COST + 1,
            ARGON2_DEFAULT_PARALLELISM
        ));
        assert!(!kdf_params_within_limits(
            ARGON2_DEFAULT_MEMORY_KIB,
            ARGON2_DEFAULT_TIME_COST,
            KDF_MAX_PARALLELISM + 1
        ));
        assert!(kdf_params_within_limits(
            KDF_MAX_MEMORY_KIB,
            KDF_MAX_TIME_COST,
            KDF_MAX_PARALLELISM
        ));
        assert!(!kdf_params_within_limits(0, 1, 1));
    }

    /// 口令绝不进入 Debug 输出：选项类型可能被 `{:?}` 打点（日志、断言
    /// 失败、panic message），Debug 面必须脱敏。
    #[test]
    fn transfer_options_debug_redacts_password() {
        let export = ExportOptions::Password {
            password: "debug-secret-123".into(),
        };
        assert!(!format!("{export:?}").contains("debug-secret-123"));
        assert!(format!("{export:?}").contains("Password"));

        let import = ImportOptions {
            password: Some("debug-secret-123".into()),
            strategy: ImportStrategy::Merge,
        };
        assert!(!format!("{import:?}").contains("debug-secret-123"));
        assert!(format!("{import:?}").contains("Merge"));
    }

    /// 读取前预检：超限文件按元数据快速拒绝（set_len 造稀疏大文件，
    /// 不实际写 16 MiB 内容），不必先整读进内存。
    #[test]
    fn precheck_transfer_file_size_rejects_oversized_by_metadata() {
        let oversized = temp_path("precheck-oversized", "bin");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(MAX_EXPORT_SIZE as u64 + 1).unwrap();
        drop(file);
        assert!(matches!(
            precheck_transfer_file_size(&oversized),
            Err(ConfigTransferError::TooLarge)
        ));
        let _ = fs::remove_file(oversized);

        let small = temp_path("precheck-small", "bin");
        fs::write(&small, b"qtray").unwrap();
        assert!(precheck_transfer_file_size(&small).is_ok());
        let _ = fs::remove_file(small);

        let missing = temp_path("precheck-missing", "bin");
        assert!(precheck_transfer_file_size(&missing).is_err());
    }

    /// 字节版写入层入口与路径版语义一致：合并并集 + 计数 + 本机配置
    /// 缺失视为空配置全额并入。
    #[test]
    fn import_from_bytes_entry_applies_strategy_and_counts() {
        let dir =
            std::env::temp_dir().join(format!("quotatray-bytes-import-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source_store = InMemoryStore::new();
        let source_vault = Vault::open(&source_store).unwrap();
        let config = sample_config(&source_vault);
        let package = dir.join("pkg.qtray-export");
        export_config_to_path_with_options(
            &config,
            &source_vault,
            None,
            None,
            &ExportOptions::Convenient,
            &package,
        )
        .unwrap();

        let target_store = InMemoryStore::new();
        let target_vault = Vault::open(&target_store).unwrap();
        let config_path = dir.join("config.json");
        let bytes = fs::read(&package).unwrap();
        let bundle = import_config_bytes_to_path_with_options(
            &bytes,
            &target_vault,
            &ImportOptions {
                password: None,
                strategy: ImportStrategy::Merge,
            },
            &config_path,
        )
        .unwrap();
        assert_eq!(bundle.counts.providers_added, config.providers.len());
        assert_eq!(bundle.counts.providers_skipped, 0);
        let restored = AppConfig::load(&config_path).unwrap();
        assert_eq!(restored.providers.len(), config.providers.len());
        let _ = fs::remove_dir_all(dir);
    }

    // ---- 导入双模：合并并集与覆盖全量替换（工单 #121）----

    /// 构造单条 native 条目配置，供双模测试拼装两台机器的不同条目集。
    fn single_native_config(vault: &Vault, id: &str, name: &str, secret: &str) -> AppConfig {
        let mut entry = ProviderEntry {
            id: id.into(),
            name: name.into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
            console_url: None,
        };
        entry.set_api_key(vault, secret).unwrap();
        AppConfig {
            providers: vec![entry],
            custom_models: BTreeMap::new(),
        }
    }

    /// 契约：既有无 options 导入入口维持「整体替换」现状语义——本机独有
    /// 条目被清、配置完全变成备份；显式 Overwrite 的带 options 入口同产物。
    #[test]
    fn legacy_import_functions_keep_whole_replace_semantics() {
        let export_path = temp_path("legacy-replace", CONFIG_EXPORT_EXTENSION);
        let config_path = temp_path("legacy-replace", "json");
        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let local_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let local = single_native_config(&local_vault, "local-only", "本机独有", "sk-local-only");
        local.save(&config_path).unwrap();

        let backup = sample_config(&source_vault);
        export_config_to_path(&backup, &source_vault, None, &export_path).unwrap();

        let bundle = import_config_to_path(&export_path, &local_vault, &config_path).unwrap();
        assert!(
            !AppConfig::load(&config_path)
                .unwrap()
                .providers
                .iter()
                .any(|entry| entry.id == "local-only"),
            "旧入口 = 整体替换：本机独有条目被清，不受 Default(Merge) 影响"
        );
        assert_eq!(
            bundle.counts,
            ImportCounts {
                providers_added: 2,
                providers_skipped: 0,
                series_added: 0,
                series_skipped: 0,
            },
            "旧入口走覆盖口径计数"
        );

        // 显式 Overwrite 的带 options 入口与旧入口同为整体替换。
        local.save(&config_path).unwrap();
        import_config_to_path_with_options(
            &export_path,
            &local_vault,
            &ImportOptions {
                strategy: ImportStrategy::Overwrite,
                ..Default::default()
            },
            &config_path,
        )
        .unwrap();
        assert!(
            !AppConfig::load(&config_path)
                .unwrap()
                .providers
                .iter()
                .any(|entry| entry.id == "local-only"),
            "显式 Overwrite 与旧入口产物一致"
        );

        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);
    }

    /// 契约：合并模——同 id 条目本机为准零覆盖（名称与密文原样保留）、
    /// 新 id 条目并入且凭据转写成功；自定义模型库按键并集、同 id 本机
    /// 定义为准；条目计数准确；bundle 本体仍是解码后的备份内容。
    #[test]
    fn merge_import_keeps_local_entries_and_adds_new_ones() {
        let export_path = temp_path("merge-union", CONFIG_EXPORT_EXTENSION);
        let config_path = temp_path("merge-union", "json");
        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let local_vault = Vault::open(&InMemoryStore::new()).unwrap();

        // 本机：keep-me（本机名称/密钥/自定义价）。
        let mut local =
            single_native_config(&local_vault, "keep-me", "本机名称", "sk-local-secret");
        local.custom_models.insert(
            "deepseek".into(),
            vec![CustomModelDef {
                id: "flash".into(),
                display: "本机定义".into(),
                peak: Some(PriceTier::full(0.1, 1.0, 2.0)),
                ..Default::default()
            }],
        );
        local.save(&config_path).unwrap();
        let local_ciphertext = local.providers[0].api_key_enc.clone();

        // 备份（另一台机器）：同 id 不同内容 + 新条目 + 键重叠/新增的自定义价。
        let mut backup =
            single_native_config(&source_vault, "keep-me", "备份名称", "sk-backup-secret");
        let new_entry =
            single_native_config(&source_vault, "new-entry", "备份新条目", "sk-backup-new");
        backup
            .providers
            .push(new_entry.providers.into_iter().next().unwrap());
        backup.custom_models.insert(
            "deepseek".into(),
            vec![CustomModelDef {
                id: "flash".into(),
                display: "备份定义".into(),
                peak: Some(PriceTier::full(9.9, 9.9, 9.9)),
                ..Default::default()
            }],
        );
        backup.custom_models.insert(
            "kimi".into(),
            vec![CustomModelDef {
                id: "moon".into(),
                display: "备份独有".into(),
                ..Default::default()
            }],
        );
        export_config_to_path(&backup, &source_vault, None, &export_path).unwrap();

        let bundle = import_config_to_path_with_options(
            &export_path,
            &local_vault,
            &ImportOptions {
                strategy: ImportStrategy::Merge,
                ..Default::default()
            },
            &config_path,
        )
        .unwrap();

        let loaded = AppConfig::load(&config_path).unwrap();
        assert_eq!(loaded.providers.len(), 2, "条目按 id 并集");
        assert_eq!(loaded.providers[0].id, "keep-me", "本机条目保持在前");

        let kept = &loaded.providers[0];
        assert_eq!(kept.name, "本机名称", "同 id 冲突以本机为准");
        assert_eq!(
            kept.api_key_enc, local_ciphertext,
            "本机条目零覆盖：密文原样保留（无需转写）"
        );
        assert_eq!(
            kept.credentials(&local_vault).unwrap().api_key.as_str(),
            "sk-local-secret"
        );

        let added = loaded
            .providers
            .iter()
            .find(|entry| entry.id == "new-entry")
            .unwrap();
        assert_eq!(
            added.credentials(&local_vault).unwrap().api_key.as_str(),
            "sk-backup-new",
            "新条目凭据已转写到本机 vault"
        );

        // 自定义模型库：同键同 id 本机定义为准，备份新键/新模型并入。
        let deepseek = loaded.custom_models.get("deepseek").unwrap();
        assert_eq!(deepseek.len(), 1);
        assert_eq!(deepseek[0].display, "本机定义");
        assert!(loaded.custom_models.contains_key("kimi"));

        assert_eq!(
            bundle.counts,
            ImportCounts {
                providers_added: 1,
                providers_skipped: 1,
                series_added: 0,
                series_skipped: 0,
            },
            "合并模条目计数准确；组合计数由 merge_usage_comparison_series 返回"
        );
        assert_eq!(
            bundle.config.providers[0].name, "备份名称",
            "bundle 本体仍是解码后的备份内容，不因策略变形"
        );

        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);
    }

    /// 契约：合并模对本机不存在的配置文件（首次恢复场景）全额并入。
    #[test]
    fn merge_import_without_local_config_adds_whole_package() {
        let export_path = temp_path("merge-empty", CONFIG_EXPORT_EXTENSION);
        let config_path = temp_path("merge-empty", "json");
        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let local_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let backup = sample_config(&source_vault);
        export_config_to_path(&backup, &source_vault, None, &export_path).unwrap();

        let bundle = import_config_to_path_with_options(
            &export_path,
            &local_vault,
            &ImportOptions {
                strategy: ImportStrategy::Merge,
                ..Default::default()
            },
            &config_path,
        )
        .unwrap();

        let loaded = AppConfig::load(&config_path).unwrap();
        assert_eq!(loaded.providers.len(), 2);
        assert_eq!(
            bundle.counts,
            ImportCounts {
                providers_added: 2,
                providers_skipped: 0,
                series_added: 0,
                series_skipped: 0,
            }
        );
        assert_eq!(
            loaded.providers[0]
                .credentials(&local_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A,
            "无本机配置时备份条目照常转写"
        );

        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);
    }

    /// 契约：覆盖模——config 与组合整体替换、全量生效计数（added = 包内
    /// 数量、skipped 恒 0）。
    #[test]
    fn overwrite_import_replaces_config_and_series_wholesale() {
        let export_path = temp_path("overwrite", CONFIG_EXPORT_EXTENSION);
        let config_path = temp_path("overwrite", "json");
        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let local_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let local = single_native_config(&local_vault, "local-only", "本机独有", "sk-local-only");
        local.save(&config_path).unwrap();

        let backup = sample_config(&source_vault);
        let comparison = vec![
            UsageComparisonSeries {
                provider_id: "native-a".into(),
                window_key: "w1".into(),
                color_slot: 0,
            },
            UsageComparisonSeries {
                provider_id: "template-b".into(),
                window_key: "w2".into(),
                color_slot: 1,
            },
        ];
        export_config_to_path_with_usage(
            &backup,
            &source_vault,
            None,
            Some(&comparison),
            &export_path,
        )
        .unwrap();

        let bundle = import_config_to_path_with_options(
            &export_path,
            &local_vault,
            &ImportOptions {
                strategy: ImportStrategy::Overwrite,
                ..Default::default()
            },
            &config_path,
        )
        .unwrap();

        let loaded = AppConfig::load(&config_path).unwrap();
        assert_eq!(
            loaded.providers.len(),
            2,
            "覆盖模整体替换：本机独有条目被清"
        );
        assert!(
            !loaded
                .providers
                .iter()
                .any(|entry| entry.id == "local-only")
        );
        assert_eq!(
            loaded.custom_models, backup.custom_models,
            "自定义模型库整体替换"
        );
        assert_eq!(
            bundle.usage_comparison_series,
            Some(comparison),
            "组合随包整体生效（调用端整体写入 settings）"
        );
        assert_eq!(
            bundle.counts,
            ImportCounts {
                providers_added: 2,
                providers_skipped: 0,
                series_added: 2,
                series_skipped: 0,
            },
            "覆盖模计数 = 全量生效、无跳过"
        );

        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);
    }

    /// 契约：比较组合并集——本机全保留（保序保色槽）、备份仅补新键、
    /// 冲突本机为准计跳过、超 4 条截断不计入任一计数。
    #[test]
    fn merge_usage_comparison_series_unions_local_first_and_caps_at_four() {
        let series = |provider_id: &str, window_key: &str, color_slot: u8| UsageComparisonSeries {
            provider_id: provider_id.into(),
            window_key: window_key.into(),
            color_slot,
        };

        // 本机 2 + 备份 3（1 键重叠）→ 本机全保留 + 新增 2，恰满 cap 4。
        let local = vec![series("p1", "w1", 0), series("p1", "w2", 1)];
        let incoming = vec![
            series("p1", "w1", 3),
            series("p2", "w3", 2),
            series("p3", "w4", 3),
        ];
        let (merged, counts) = merge_usage_comparison_series(&local, &incoming);
        let merged_keys: Vec<(&str, &str)> = merged
            .iter()
            .map(|item| (item.provider_id.as_str(), item.window_key.as_str()))
            .collect();
        assert_eq!(
            merged_keys,
            vec![("p1", "w1"), ("p1", "w2"), ("p2", "w3"), ("p3", "w4")],
            "本机在前保序，备份新键按序追加"
        );
        assert_eq!(merged[0].color_slot, 0, "本机色槽原样保留");
        assert_eq!(merged[1].color_slot, 1);
        assert_eq!(
            counts,
            ImportCounts {
                providers_added: 0,
                providers_skipped: 0,
                series_added: 2,
                series_skipped: 1,
            }
        );

        // cap 截断：本机 3 + 备份 2（不重叠）→ 只并入第一条，第二条被截断
        // （不计新增也不计跳过）；备份抢本机色槽时修复到空闲槽。
        let local = vec![
            series("a", "w", 0),
            series("b", "w", 1),
            series("c", "w", 2),
        ];
        let incoming = vec![series("d", "w", 0), series("e", "w", 1)];
        let (merged, counts) = merge_usage_comparison_series(&local, &incoming);
        assert_eq!(merged.len(), MAX_USAGE_COMPARISON_SERIES);
        assert!(merged.iter().any(|item| item.provider_id == "d"));
        assert!(
            !merged.iter().any(|item| item.provider_id == "e"),
            "超出 4 条上限的备份键被截断"
        );
        assert_eq!(merged[3].color_slot, 3, "备份冲突色槽修复到空闲槽");
        assert_eq!(
            counts,
            ImportCounts {
                providers_added: 0,
                providers_skipped: 0,
                series_added: 1,
                series_skipped: 0,
            }
        );
    }

    /// 契约：ImportOptions 默认合并（保守）；T-12 旧序列化形态（无
    /// strategy 字段）反序列化回退合并；两模 serde roundtrip。
    #[test]
    fn import_options_default_strategy_is_merge_and_serde_compatible() {
        assert_eq!(ImportStrategy::default(), ImportStrategy::Merge);
        assert_eq!(ImportOptions::default().strategy, ImportStrategy::Merge);

        let legacy: ImportOptions = serde_json::from_str(r#"{"password":"12345678"}"#).unwrap();
        assert_eq!(legacy.strategy, ImportStrategy::Merge);
        assert_eq!(legacy.password.as_deref(), Some("12345678"));

        let overwrite: ImportOptions =
            serde_json::from_str(r#"{"password":null,"strategy":"Overwrite"}"#).unwrap();
        assert_eq!(overwrite.strategy, ImportStrategy::Overwrite);
    }

    /// 契约：纯解码入口不接触本机状态，计数恒为零值。
    #[test]
    fn decode_only_import_returns_zero_counts() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let bytes = export_config(&sample_config(&source_vault), &source_vault, None).unwrap();
        let bundle = import_config(&bytes, &Vault::open(&InMemoryStore::new()).unwrap()).unwrap();
        assert_eq!(bundle.counts, ImportCounts::default());
    }
}
