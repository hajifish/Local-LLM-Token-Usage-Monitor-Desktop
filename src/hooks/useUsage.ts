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
      // 先注册一次性事件监听，确保不遗漏后端发出的 usage-updated
      const eventPromise = new Promise<UsageSummary>((resolve) => {
        listen<UsageSummary>('usage-updated', (event) => {
          resolve(event.payload);
        });
      });

      // 通知后端立即轮询供应商
      await invoke('trigger_refresh');

      // 等待 usage-updated 事件（10 秒超时兜底）
      const timeout = new Promise<UsageSummary | null>((resolve) =>
        setTimeout(() => resolve(null), 10000),
      );

      const result = await Promise.race([eventPromise, timeout]);
      if (result) {
        setSummary(result);
      }
      await fetchUsage(); // 最终确认
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
