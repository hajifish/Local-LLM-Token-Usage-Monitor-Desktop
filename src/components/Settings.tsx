import { useState, useEffect } from 'react';
import type { CSSProperties } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig, ConfigHealth, ProviderConfig } from '../types';

interface SettingsProps {
  onBack: () => void;
}

const PROVIDER_OPTIONS = [
  { value: 'OpenAI', label: 'OpenAI' },
  { value: 'Codex', label: 'Codex (ChatGPT)' },
  { value: 'Anthropic', label: 'Anthropic' },
  { value: 'Claude Code', label: 'Claude Code' },
  { value: 'DeepSeek', label: 'DeepSeek' },
  { value: 'Kimi API', label: 'Kimi API' },
  { value: 'Kimi Code', label: 'Kimi Code' },
  { value: 'Zhipu', label: '智谱' },
];

/** 无需用户配置 API Key，凭证从 CLI 配置文件自动读取的供应商类型。 */
const CLI_OAUTH_PROVIDERS = new Set(['Codex', 'Claude Code', 'Kimi Code']);

/** 刷新间隔预设选项：秒数 → 显示文案。 */
const REFRESH_OPTIONS: { value: number; label: string }[] = [
  { value: 30, label: '30秒' },
  { value: 60, label: '1分钟' },
  { value: 120, label: '2分钟' },
  { value: 300, label: '5分钟' },
  { value: 600, label: '10分钟' },
];

/** 「配置已升级为加密存储」为一次性提示，展示过后写入本地存储不再打扰。 */
const MIGRATED_NOTICE_KEY = 'llmtm.migratedNoticeShown';

/**
 * 健康提示横幅样式。
 *
 * 刻意使用 inline style：全局样式表由其他改动占用，避免产生冲突。
 */
const BANNER_BASE: CSSProperties = {
  margin: '12px 0',
  padding: '12px 14px',
  borderRadius: 8,
  fontSize: 13,
  lineHeight: 1.7,
  border: '1px solid',
  position: 'relative',
  paddingRight: 36,
};

const BANNER_TONE: Record<'danger' | 'warn' | 'info', CSSProperties> = {
  danger: {
    ...BANNER_BASE,
    background: 'rgba(255, 77, 79, 0.10)',
    borderColor: 'rgba(255, 77, 79, 0.55)',
    color: '#ffb3b3',
  },
  warn: {
    ...BANNER_BASE,
    background: 'rgba(250, 173, 20, 0.10)',
    borderColor: 'rgba(250, 173, 20, 0.55)',
    color: '#ffe0a3',
  },
  info: {
    ...BANNER_BASE,
    background: 'rgba(64, 150, 255, 0.10)',
    borderColor: 'rgba(64, 150, 255, 0.55)',
    color: '#bcd9ff',
  },
};

const BANNER_TITLE: CSSProperties = {
  display: 'block',
  fontWeight: 700,
  marginBottom: 4,
  fontSize: 13.5,
};

const BANNER_CLOSE: CSSProperties = {
  position: 'absolute',
  top: 8,
  right: 8,
  border: 'none',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  fontSize: 16,
  lineHeight: 1,
  opacity: 0.7,
  padding: '2px 6px',
};

/** 从绝对路径中取出文件名，仅用于展示（不暴露完整目录）。 */
function basename(p: string): string {
  const parts = p.split(/[\\/]/);
  return parts[parts.length - 1] || p;
}

/** 根据 provider 标识取默认显示名（下拉框 label）；未知类型兜底返回标识本身。 */
function providerLabel(name: string): string {
  return PROVIDER_OPTIONS.find((o) => o.value === name)?.label || name;
}

function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [health, setHealth] = useState<ConfigHealth | null>(null);
  const [dismissed, setDismissed] = useState<Record<string, boolean>>({});
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState('');
  /** 正在确认删除的供应商索引；null 表示未在确认中 */
  const [deletingIndex, setDeletingIndex] = useState<number | null>(null);

  /** 刷新健康状态（保存后重新拉取，以便展示备份文件名）。 */
  const refreshHealth = () =>
    invoke<ConfigHealth>('get_config_health').then(setHealth).catch(console.error);

  useEffect(() => {
    let alive = true;
    // 并行拉取配置与健康状态；健康状态获取失败不阻塞设置页使用
    Promise.all([
      invoke<AppConfig>('get_config'),
      invoke<ConfigHealth>('get_config_health').catch(() => null),
    ])
      .then(([cfg, h]) => {
        if (!alive) return;
        setConfig(cfg);
        setHealth(h);
        if (h?.migrated_from_plaintext) {
          if (localStorage.getItem(MIGRATED_NOTICE_KEY)) {
            // 已展示过，不再重复提示
            setDismissed((d) => ({ ...d, migrated: true }));
          } else {
            localStorage.setItem(MIGRATED_NOTICE_KEY, '1');
          }
        }
      })
      .catch(console.error);
    return () => {
      alive = false;
    };
  }, []);

  const dismiss = (key: string) => setDismissed((d) => ({ ...d, [key]: true }));

  const updateProvider = (index: number, field: keyof ProviderConfig, value: string | boolean) => {
    if (!config) return;
    const newProviders = [...config.providers];
    newProviders[index] = { ...newProviders[index], [field]: value };
    setConfig({ ...config, providers: newProviders });
  };

  const addProvider = () => {
    if (!config) return;
    const def = PROVIDER_OPTIONS[0];
    // 别名只承载用户自定义值：新增时不预填，展示时由后端回退到供应商默认名
    setConfig({
      ...config,
      providers: [
        ...config.providers,
        { name: def.value, api_key: '', enabled: true, platform_token: null, alias: null },
      ],
    });
  };

  /**
   * 切换供应商类型：别名只承载用户自定义值。
   * - 别名为空、或恰好等于旧类型的默认名（历史预填残留）→ 置空，让后端回退到新类型默认名；
   * - 用户已输入自定义别名 → 保留，不因切换类型而丢失。
   */
  const handleProviderNameChange = (index: number, newName: string) => {
    if (!config) return;
    const old = config.providers[index];
    const oldAlias = (old.alias || '').trim();
    const keepAlias = oldAlias !== '' && oldAlias !== providerLabel(old.name);
    const newProviders = [...config.providers];
    newProviders[index] = { ...old, name: newName, alias: keepAlias ? old.alias : null };
    setConfig({ ...config, providers: newProviders });
  };

  const removeProvider = (index: number) => {
    if (!config) return;
    const newProviders = config.providers.filter((_, i) => i !== index);
    setConfig({ ...config, providers: newProviders });
    setDeletingIndex(null);
  };

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    setMessage('');
    try {
      await invoke('save_config', { newConfig: config });
      setMessage('保存成功');
      // 保存可能触发「覆盖前备份」，重新拉取健康状态以展示备份文件名
      refreshHealth();
    } catch (e) {
      // 后端已将内部错误映射为中文用户可读消息，技术细节仅写日志
      setMessage(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  if (!config) {
    return <div className="settings loading">加载中...</div>;
  }

  const showUnreadable = !!health?.unreadable && !dismissed.unreadable;
  const showDeferred = !!health?.migration_deferred && !dismissed.deferred;
  const showMigrated = !!health?.migrated_from_plaintext && !dismissed.migrated;

  return (
    <div className="settings">
      <div className="settings-header">
        <button className="back-btn" onClick={onBack}>
          ← 返回
        </button>
        <h2>设置</h2>
        <button className="save-btn-header" onClick={handleSave} disabled={saving}>
          {saving ? '保存中...' : '保存'}
        </button>
      </div>
      {message && <div className="save-toast">{message}</div>}

      <div className="settings-body">
        {/* 配置健康提示 */}
        {showUnreadable && (
          <div style={BANNER_TONE.danger} role="alert">
            <span style={BANNER_TITLE}>⚠ 检测到本机无法解密的配置文件</span>
            {health?.backup_path ? (
              <span>
                原文件已备份为 <strong>{basename(health.backup_path)}</strong>
                ，继续保存将写入新配置；如需恢复原密钥，请回到原设备或联系支持。
              </span>
            ) : (
              <span>
                原文件目前未被修改；若继续保存，会先自动将原文件备份后再写入新配置。
                如需恢复原密钥，请回到原设备或联系支持。
              </span>
            )}
            <br />
            <span>当前页面展示的是默认空配置（并非你的密钥已丢失）。</span>
            <button style={BANNER_CLOSE} onClick={() => dismiss('unreadable')} aria-label="关闭提示">
              ×
            </button>
          </div>
        )}

        {showDeferred && (
          <div style={BANNER_TONE.warn}>
            <span style={BANNER_TITLE}>配置的加密升级已暂缓</span>
            <span>
              当前无法获取本机稳定的硬件标识，为避免将密钥绑定到不稳定的标识上，
              明文配置的加密升级已暂缓；应用功能不受影响，下次启动若标识恢复可用会自动完成升级。
            </span>
            <button style={BANNER_CLOSE} onClick={() => dismiss('deferred')} aria-label="关闭提示">
              ×
            </button>
          </div>
        )}

        {showMigrated && (
          <div style={BANNER_TONE.info}>
            <span style={BANNER_TITLE}>配置已升级为机器绑定加密存储</span>
            <span>
              配置已升级为机器绑定的加密存储（原明文已安全擦除）。
              注意：回退到旧版本将无法读取该配置，换机或重装系统后需重新录入密钥。
            </span>
            <button style={BANNER_CLOSE} onClick={() => dismiss('migrated')} aria-label="关闭提示">
              ×
            </button>
          </div>
        )}

        {/* 全局设置 */}
        <div className="settings-section">
          <div className="global-settings-row">
            <img src="/logo-hd.png" alt="Logo" className="global-logo" />
            <div className="refresh-picker">
              <span className="refresh-label">刷新间隔</span>
              <div className="refresh-options">
                {REFRESH_OPTIONS.map((opt) => (
                  <button
                    key={opt.value}
                    className={`refresh-chip ${config.refresh_interval === opt.value ? 'active' : ''}`}
                    onClick={() => setConfig({ ...config, refresh_interval: opt.value })}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* 服务商配置 */}
        <div className="settings-section">
          <h3>服务商</h3>
          {config.providers.map((p, i) => (
            <div key={`${p.name}-${p.alias || i}`} className="provider-config">
              <div className="provider-config-header">
                <select value={p.name} onChange={(e) => handleProviderNameChange(i, e.target.value)}>
                  {PROVIDER_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                <label className="toggle">
                  <input
                    type="checkbox"
                    checked={p.enabled}
                    onChange={(e) => updateProvider(i, 'enabled', e.target.checked)}
                  />
                </label>
                {deletingIndex === i ? (
                  <>
                    <span className="delete-confirm-text">确定?</span>
                    <button className="remove-btn remove-btn-confirm" onClick={() => removeProvider(i)}>确定</button>
                    <button className="cancel-btn" onClick={() => setDeletingIndex(null)}>取消</button>
                  </>
                ) : (
                  <button className="remove-btn" onClick={() => setDeletingIndex(i)}>删除</button>
                )}
              </div>
              <div className="provider-fields">
                <input
                  type="text"
                  placeholder={`别名（默认「${providerLabel(p.name)}」）`}
                  value={p.alias || ''}
                  onChange={(e) => updateProvider(i, 'alias', e.target.value)}
                />
                {CLI_OAUTH_PROVIDERS.has(p.name) ? (
                  <span className="ds-hint">
                    {p.name === 'Codex'
                      ? '自动从 ~/.codex/auth.json 读取凭证'
                      : p.name === 'Claude Code'
                        ? '自动从 ~/.claude/credentials.json 读取凭证'
                        : '自动从 ~/.kimi-code/config.toml 读取凭证'}
                  </span>
                ) : (
                  <input
                    type="password"
                    placeholder="API Key"
                    value={p.api_key}
                    onChange={(e) => updateProvider(i, 'api_key', e.target.value)}
                  />
                )}
                {(p.name === 'OpenAI' || p.name === 'Anthropic') && (
                  <span className="ds-hint">需组织管理员密钥（Admin Key）</span>
                )}
                {p.name === 'DeepSeek' && (
                  <>
                    <input
                      type="password"
                      placeholder="平台 Token（可选，查看今日用量）"
                      value={p.platform_token || ''}
                      onChange={(e) => updateProvider(i, 'platform_token', e.target.value)}
                    />
                    <span className="ds-hint">从 platform.deepseek.com 登录后获取</span>
                  </>
                )}
              </div>
            </div>
          ))}
          <button className="add-btn" onClick={addProvider}>
            + 添加服务商
          </button>
        </div>
      </div>
    </div>
  );
}

export default Settings;
