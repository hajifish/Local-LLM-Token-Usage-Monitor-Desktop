import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ProviderConfig {
  name: string;
  api_key: string;
  enabled: boolean;
  platform_token?: string | null;
  alias?: string | null;
}

interface AppConfig {
  providers: ProviderConfig[];
  refresh_interval: number;
}

interface SettingsProps {
  onBack: () => void;
}

const PROVIDER_OPTIONS = [
  { value: "Zhipu", label: "智谱" },
  { value: "DeepSeek", label: "DeepSeek" },
  { value: "Kimi", label: "Kimi" },
  { value: "OpenAI", label: "OpenAI" },
  { value: "Anthropic", label: "Anthropic" },
];

function Settings({ onBack }: SettingsProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then(setConfig)
      .catch(console.error);
  }, []);

  const updateProvider = (index: number, field: keyof ProviderConfig, value: string | boolean) => {
    if (!config) return;
    const newProviders = [...config.providers];
    newProviders[index] = { ...newProviders[index], [field]: value };
    setConfig({ ...config, providers: newProviders });
  };

  const addProvider = () => {
    if (!config) return;
    const def = PROVIDER_OPTIONS[0];
    setConfig({
      ...config,
      providers: [...config.providers, { name: def.value, api_key: "", enabled: true, platform_token: null, alias: def.label }],
    });
  };

  const removeProvider = (index: number) => {
    if (!config) return;
    const p = config.providers[index];
    const label = p.alias || PROVIDER_OPTIONS.find(o => o.value === p.name)?.label || p.name;
    if (!confirm(`确定要删除「${label}」的配置吗？`)) return;
    const newProviders = config.providers.filter((_, i) => i !== index);
    setConfig({ ...config, providers: newProviders });
  };

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    setMessage("");
    try {
      await invoke("save_config", { newConfig: config });
      setMessage("保存成功");
    } catch (e) {
      setMessage(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  if (!config) {
    return <div className="settings loading">加载中...</div>;
  }

  return (
    <div className="settings">
      <div className="settings-header">
        <button className="back-btn" onClick={onBack}>← 返回</button>
        <h2>设置</h2>
      </div>

      {/* Global settings card: logo + refresh interval */}
      <div className="settings-section">
        <div className="global-settings-card logo-row">
          <div className="logo-display">
            <img src="/logo-hd.png" alt="Logo" className="logo-full" />
          </div>
          <div className="logo-settings">
            <label className="field-label">
              刷新间隔（秒）
              <input
                type="number"
                min={30}
                value={config.refresh_interval}
                onChange={(e) =>
                  setConfig({ ...config, refresh_interval: Math.max(30, parseInt(e.target.value) || 30) })
                }
              />
            </label>
          </div>
        </div>
      </div>

      {/* Provider config section */}
      <div className="settings-section">
        <h3>服务商配置</h3>
        {config.providers.map((p, i) => (
          <div key={i} className="provider-config">
            <div className="provider-config-header">
              <select value={p.name} onChange={(e) => updateProvider(i, "name", e.target.value)}>
                {PROVIDER_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={p.enabled}
                  onChange={(e) => updateProvider(i, "enabled", e.target.checked)}
                />
                启用
              </label>
              <button className="remove-btn" onClick={() => removeProvider(i)}>删除</button>
            </div>
            <input
              type="text"
              placeholder="别名（用于区分同服务商多账号）"
              value={p.alias || ""}
              onChange={(e) => updateProvider(i, "alias", e.target.value)}
            />
            <input
              type="password"
              placeholder="API Key"
              value={p.api_key}
              onChange={(e) => updateProvider(i, "api_key", e.target.value)}
              style={{ marginTop: "8px" }}
            />
            {(p.name === "OpenAI" || p.name === "Anthropic") && (
              <span className="ds-hint">
                需组织管理员密钥（Admin Key），普通 API Key 无法查询用量/花费
              </span>
            )}
            {p.name === "DeepSeek" && (
              <>
                <input
                  type="password"
                  placeholder="平台 Token（可选，用于查看今日用量）"
                  value={p.platform_token || ""}
                  onChange={(e) => updateProvider(i, "platform_token", e.target.value)}
                  style={{ marginTop: "8px" }}
                />
                <span className="ds-hint">
                  从 platform.deepseek.com 登录后获取
                </span>
              </>
            )}
          </div>
        ))}
        <button className="add-btn" onClick={addProvider}>+ 添加服务商</button>
      </div>

      <div className="settings-actions">
        <button className="save-btn" onClick={handleSave} disabled={saving}>
          {saving ? "保存中..." : "保存配置"}
        </button>
        {message && <span className="save-message">{message}</span>}
      </div>
    </div>
  );
}

export default Settings;
