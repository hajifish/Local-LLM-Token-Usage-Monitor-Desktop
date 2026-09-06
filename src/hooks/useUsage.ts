import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UsageSummary } from '../types';

export type { BalanceData, QuotaInfo, UsageData, ProviderStatus, UsageSummary } from '../types';

export function useUsage() {
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchUsage = useCallback(async () => {
    try {
      setError(null);
      const data = await invoke<UsageSummary>('get_usage');
      setSummary(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      // 通知后端立即轮询供应商
      await invoke('trigger_refresh');
      // 等待轮询完成（后端完成后会 emit usage-updated 事件，
      // 但 get_usage 读取的是同一份共享状态，稍等即可拿到新数据）
      await new Promise((r) => setTimeout(r, 1500));
      await fetchUsage();
    } catch {
      await fetchUsage();
    }
  }, [fetchUsage]);

  useEffect(() => {
    fetchUsage();

    const unlisten = listen<UsageSummary>('usage-updated', (event) => {
      setSummary(event.payload);
      setLoading(false);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [fetchUsage]);

  return { summary, loading, error, refresh };
}
