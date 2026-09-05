import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

export function useUsage() {
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchUsage = useCallback(async () => {
    try {
      setError(null);
      const data = await invoke<UsageSummary>("get_usage");
      setSummary(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    await fetchUsage();
  }, [fetchUsage]);

  useEffect(() => {
    fetchUsage();

    const unlisten = listen<UsageSummary>("usage-updated", (event) => {
      setSummary(event.payload);
      setLoading(false);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [fetchUsage]);

  return { summary, loading, error, refresh };
}
