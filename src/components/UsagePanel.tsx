import type { ProviderStatus, QuotaInfo, UsageSummary } from "../hooks/useUsage";

// Official brand logos downloaded from provider websites (stored in public/providers/)
const BRAND_LOGOS: Record<string, string> = {
  DeepSeek: "/providers/deepseek.png",
  Zhipu: "/providers/zhipu.png",
  Kimi: "/providers/kimi.png",
  OpenAI: "/providers/openai.png",
  Anthropic: "/providers/anthropic.png",
};

function BrandLogo({ name }: { name: string }) {
  const src = BRAND_LOGOS[name];
  if (src) {
    return <img src={src} alt={name} className="brand-logo-img" />;
  }
  // Fallback: colored letter badge
  const initial = name.charAt(0).toUpperCase();
  return <span className="brand-logo">{initial}</span>;
}

// 0 = ok, 1 = yellow (low), 2 = red (empty)
function warningLevel(p: ProviderStatus): number {
  if (!p.balance) return 0;
  const v = p.balance.available_balance;
  if (p.balance.currency === "USD") return 0;
  if (v <= 0) return 2;
  if (p.balance.currency === "%") return v < 5 ? 1 : 0;
  return v < 10 ? 1 : 0;
}

function ProviderCard({ p }: { p: ProviderStatus }) {
  if (!p.enabled) {
    return (
      <div className="p-card p-card-disabled">
        <div className="p-card-head">
          <span className="p-card-brand">
            <BrandLogo name={p.name} />
            <span className="p-card-name">{p.alias}</span>
          </span>
          <span className="p-card-badge gray">已禁用</span>
        </div>
      </div>
    );
  }

  if (p.error) {
    return (
      <div className="p-card p-card-error">
        <div className="p-card-head">
          <span className="p-card-brand">
            <BrandLogo name={p.name} />
            <span className="p-card-name">{p.alias}</span>
          </span>
          <span className="p-card-badge red">异常</span>
        </div>
        <p className="p-card-error-msg">{p.error}</p>
      </div>
    );
  }

  const { balance, usage, quota_infos } = p;
  const isPercent = balance?.currency === "%";
  const isUsd = balance?.currency === "USD";
  // For percent-based providers (e.g. Zhipu), prefix the label with the primary quota name
  const primaryQuotaName =
    isPercent && quota_infos && quota_infos.length > 0 ? quota_infos[0].name : null;
  const warn = warningLevel(p);
  const warnClass = warn === 2 ? "p-card-warn-red" : warn === 1 ? "p-card-warn-yellow" : "";

  return (
    <div className={`p-card ${warnClass}`.trim()}>
      {/* Card header with brand logo */}
      <div className="p-card-head">
        <span className="p-card-brand">
          <BrandLogo name={p.name} />
          <span className="p-card-name">{p.alias}</span>
        </span>
      </div>

      {/* Main display: big number */}
      {balance && (
        <div className="p-card-main">
          <span className={`p-card-big ${isUsd ? "accent-blue" : "accent-green"}`}>
            {isPercent
              ? `${balance.available_balance.toFixed(0)}%`
              : isUsd
                ? `$${balance.available_balance.toFixed(2)}`
                : `¥${balance.available_balance.toFixed(2)}`}
          </span>
          <span className="p-card-label">
            {isPercent
              ? `${primaryQuotaName ? primaryQuotaName + " " : ""}配额剩余`
              : isUsd
                ? "本月已花费"
                : "剩余余额"}
          </span>
        </div>
      )}

      {/* Details section */}
      <div className="p-card-details">
        {/* Balance breakdown */}
        {balance && !isPercent && !isUsd && (
          <div className="p-card-detail-row">
            <span className="detail-key">赠金</span>
            <span className="detail-val">¥{balance.voucher_balance.toFixed(2)}</span>
          </div>
        )}
        {balance && !isPercent && !isUsd && (
          <div className="p-card-detail-row">
            <span className="detail-key">现金</span>
            <span className="detail-val">¥{balance.cash_balance.toFixed(2)}</span>
          </div>
        )}

        {/* Today usage */}
        {usage && usage.total_tokens > 0 && (
          <>
            <div className="p-card-detail-row">
              <span className="detail-key">今日 Token</span>
              <span className="detail-val">{usage.total_tokens.toLocaleString()}</span>
            </div>
            {usage.today_requests != null && usage.today_requests > 0 && (
              <div className="p-card-detail-row">
                <span className="detail-key">调用次数</span>
                <span className="detail-val">{usage.today_requests.toLocaleString()}</span>
              </div>
            )}
          </>
        )}

        {/* Quota infos */}
        {quota_infos && quota_infos.map((q: QuotaInfo, i: number) => (
          <div className="p-card-detail-row" key={i}>
            <span className="detail-key">
              {q.plan_label && <span className="quota-plan-tag">{q.plan_label}</span>}
              {q.name}
            </span>
            <span className="detail-val">
              {q.remaining_percent.toFixed(0)}%
              {q.reset_time && <span className="reset-hint">{q.reset_time}</span>}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

interface UsagePanelProps {
  summary: UsageSummary | null;
  loading: boolean;
  error: string | null;
  refresh: () => void;
}

function UsagePanel({ summary, loading, error, refresh }: UsagePanelProps) {
  if (loading) {
    return (
      <div className="usage-panel loading">
        <div className="loading-spinner" />
        <span className="loading-text">加载中...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="usage-panel empty">
        <div className="empty-icon">!</div>
        <p className="empty-title">获取数据失败</p>
        <p className="empty-hint">{error}</p>
        <button className="action-btn" onClick={refresh}>重试</button>
      </div>
    );
  }

  if (!summary || summary.providers.length === 0) {
    return (
      <div className="usage-panel empty">
        <div className="empty-icon">+</div>
        <p className="empty-title">暂无服务商</p>
        <p className="empty-hint">请前往设置添加 API Key</p>
      </div>
    );
  }

  return (
    <div className="usage-panel">
      <div className="card-grid">
        {summary.providers.map((p, i) => (
          <ProviderCard key={`${p.name}-${i}`} p={p} />
        ))}
      </div>
    </div>
  );
}

export default UsagePanel;
