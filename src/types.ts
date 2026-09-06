/**
 * Shared type definitions — single source of truth.
 * 字段名与类型须与 Rust models.rs serde 输出严格一致（snake_case）。
 * 此文件保持零依赖，避免循环导入。
 */

export interface BalanceData {
  provider: string;
  available_balance: number;
  voucher_balance: number;
  cash_balance: number;
  total_balance: number;
  currency: string;
}

export interface QuotaInfo {
  name: string;
  used: number;
  total: number;
  current_value: number | null;
  remaining: number | null;
  remaining_percent: number;
  reset_time: string | null;
  plan_label: string | null;
}

export interface UsageData {
  provider: string;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  today_cost: number | null;
  today_requests: number | null;
}

export interface ProviderStatus {
  name: string;
  alias: string;
  enabled: boolean;
  balance: BalanceData | null;
  usage: UsageData | null;
  error: string | null;
  quota_infos: QuotaInfo[] | null;
}

export interface UsageSummary {
  providers: ProviderStatus[];
  total_balance: number;
  last_updated: string;
}

export interface ProviderConfig {
  name: string;
  api_key: string;
  enabled: boolean;
  platform_token?: string | null;
  alias?: string | null;
}

export interface AppConfig {
  providers: ProviderConfig[];
  refresh_interval: number;
}

/**
 * 配置健康状态 —— 与 Rust `config::ConfigHealth` 严格一致（snake_case）。
 * 本次启动加载配置文件时发生的关键事件，用于在设置页明确提示用户。
 */
export interface ConfigHealth {
  /** 配置文件存在，但本机无法解密/读取（换机、系统重装、篡改或损坏）。 */
  unreadable: boolean;
  /** 覆盖前备份守卫生成的 .bak 文件路径；仅在 unreadable 状态下执行保存后才有值。 */
  backup_path: string | null;
  /** 明文→密文迁移被暂缓（当前仅能取得兜底机器标识），下次启动会重试。 */
  migration_deferred: boolean;
  /** 本次启动已完成明文→密文迁移（原明文已被加密信封安全替换）。 */
  migrated_from_plaintext: boolean;
}
