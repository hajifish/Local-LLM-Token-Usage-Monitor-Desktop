use crate::models::{AppConfig, ProviderStatus, UsageSummary};
use crate::providers;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Emitter,
};
use tokio::sync::{Notify, RwLock};
use tokio::time::Duration;

pub struct Scheduler {
    app_handle: AppHandle,
    config: Arc<RwLock<AppConfig>>,
    last_summary: Arc<RwLock<UsageSummary>>,
    notify: Arc<Notify>,
}

impl Scheduler {
    pub fn new_with_notify(
        app_handle: AppHandle,
        config: Arc<RwLock<AppConfig>>,
        last_summary: Arc<RwLock<UsageSummary>>,
        notify: Arc<Notify>,
    ) -> Self {
        Self {
            app_handle,
            config,
            last_summary,
            notify,
        }
    }

    pub fn start(&self) {
        let app_handle = self.app_handle.clone();
        let config = self.config.clone();
        let last_summary = self.last_summary.clone();
        let notify = self.notify.clone();

        tauri::async_runtime::spawn(async move {
            // 启动时立即执行一次
            poll_providers(&app_handle, &config, &last_summary).await;

            // 使用短间隔检查，支持配置变更立即触发
            let mut last_poll = std::time::Instant::now();
            loop {
                // 每 5 秒检查一次
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    _ = notify.notified() => {
                        // 配置变更，立即轮询
                        last_poll = std::time::Instant::now() - Duration::from_secs(999);
                    }
                }

                let interval_secs = {
                    let cfg = config.read().await;
                    cfg.refresh_interval.max(30)
                };

                if last_poll.elapsed().as_secs() >= interval_secs {
                    poll_providers(&app_handle, &config, &last_summary).await;
                    last_poll = std::time::Instant::now();
                }
            }
        });
    }
}

async fn poll_providers(
    app_handle: &AppHandle,
    config: &Arc<RwLock<AppConfig>>,
    last_summary: &Arc<RwLock<UsageSummary>>,
) {
    log::info!("Starting provider data refresh");
    let cfg = {
        let c = config.read().await;
        c.clone()
    };
    // 读锁已释放，后续网络请求不持锁
    let mut provider_statuses = Vec::new();
    let mut total_balance = 0.0;

    for provider_cfg in &cfg.providers {
        let display_alias = provider_cfg
            .alias
            .clone()
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| provider_cfg.name.clone());

        // Codex / Claude Code 无需 API Key（凭证从 CLI 配置文件读取），跳过空 key 检查
        let needs_api_key = !matches!(provider_cfg.name.as_str(), "Codex" | "Claude Code");
        if !provider_cfg.enabled || (needs_api_key && provider_cfg.api_key.is_empty()) {
            provider_statuses.push(ProviderStatus {
                name: provider_cfg.name.clone(),
                alias: display_alias,
                enabled: provider_cfg.enabled,
                balance: None,
                usage: None,
                error: None,
                quota_infos: None,
            });
            continue;
        }

        if let Some(provider) = providers::create_provider(
            &provider_cfg.name,
            &provider_cfg.api_key,
            provider_cfg.platform_token.as_deref(),
        ) {
            match provider.fetch_balance().await {
                Ok(balance) => {
                    // 正向白名单：仅 CNY 余额计入 ¥ 总额（% 配额与 USD 成本均排除，且防未来 EUR 等被误并入）
                    if balance.is_cny() {
                        total_balance += balance.available_balance;
                    }
                    let quota_infos = provider.fetch_quota_infos().await.ok().flatten();
                    let usage = provider.fetch_usage().await.ok().flatten();
                    provider_statuses.push(ProviderStatus {
                        name: provider.name().to_string(),
                        alias: display_alias.clone(),
                        enabled: true,
                        balance: Some(balance),
                        usage,
                        error: None,
                        quota_infos,
                    });
                }
                Err(e) => {
                    log::warn!("Failed to fetch {} data: {}", provider_cfg.name, e);
                    provider_statuses.push(ProviderStatus {
                        name: provider.name().to_string(),
                        alias: display_alias.clone(),
                        enabled: true,
                        balance: None,
                        usage: None,
                        error: Some(e.to_string()),
                        quota_infos: None,
                    });
                }
            }
        }
    }

    let summary = UsageSummary {
        providers: provider_statuses,
        total_balance,
        last_updated: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    // 更新共享状态
    {
        let mut s = last_summary.write().await;
        *s = summary.clone();
    }

    // 更新托盘菜单和 tooltip
    update_tray_menu(app_handle, &summary);

    // 更新托盘图标警告打点（余量不足时显示红/黄点）
    crate::tray::update_tray_badge(app_handle, &summary);

    // 推送给前端
    let _ = app_handle.emit("usage-updated", &summary);
}

fn update_tray_menu(app_handle: &AppHandle, summary: &UsageSummary) {
    if let Some(tray) = app_handle.tray_by_id("main_tray") {
        // --- 构建 tooltip（鼠标悬停提示） ---
        let mut tooltip_lines = vec!["LLM Token Monitor".to_string()];
        for p in &summary.providers {
            if let Some(ref balance) = p.balance {
                if balance.is_percent() {
                    tooltip_lines.push(format!(
                        "{}: 剩余 {:.0}%",
                        p.alias, balance.available_balance
                    ));
                } else if balance.is_usd() {
                    tooltip_lines.push(format!(
                        "{}: 本月 ${:.2}",
                        p.alias, balance.available_balance
                    ));
                } else {
                    tooltip_lines.push(format!(
                        "{}: ¥{:.2} (赠金 ¥{:.2} / 现金 ¥{:.2})",
                        p.alias,
                        balance.available_balance,
                        balance.voucher_balance,
                        balance.cash_balance
                    ));
                }
            } else if p.error.is_some() {
                tooltip_lines.push(format!("{}: 获取失败", p.alias));
            }
        }
        tooltip_lines.push(format!("总余额: ¥{:.2}", summary.total_balance));
        tooltip_lines.push(format!("更新: {}", summary.last_updated));
        let _ = tray.set_tooltip(Some(&tooltip_lines.join("\n")));

        // --- 动态构建主菜单（供应商状态内联展开，不再用二级子菜单） ---
        let Ok(menu) = Menu::new(app_handle) else {
            return;
        };

        // 关键项（refresh/settings/quit）append 必须全部成功才装配菜单，
        // 否则会装上缺少“退出”的菜单（ActivationPolicy::Accessory 下托盘退出是唯一出口）。
        let mut critical_ok = true;

        // 0) 打开主页面（顶部第一项）
        let Ok(open_item) =
            MenuItem::with_id(app_handle, "open-main-window", "打开主页面", true, None::<&str>)
        else {
            return;
        };
        critical_ok &= menu.append(&open_item).is_ok();
        let Ok(separator_open) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator_open).is_ok();

        // 1) 刷新数据
        let Ok(refresh_item) =
            MenuItem::with_id(app_handle, "refresh", "刷新数据", true, None::<&str>)
        else {
            return;
        };
        critical_ok &= menu.append(&refresh_item).is_ok();
        let Ok(separator_top) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator_top).is_ok();

        // 2) 供应商状态条目（disabled，不可点击），上限 50 条；id 加序号防别名重复冲突
        const MAX_PROVIDER_ITEMS: usize = 50;
        for (i, p) in summary
            .providers
            .iter()
            .take(MAX_PROVIDER_ITEMS)
            .enumerate()
        {
            let text = if let Some(ref balance) = p.balance {
                if balance.is_percent() {
                    format!("{}: 剩余 {:.0}%", p.alias, balance.available_balance)
                } else if balance.is_usd() {
                    format!("{}: 本月 ${:.2}", p.alias, balance.available_balance)
                } else {
                    format!("{}: ¥{:.2}", p.alias, balance.available_balance)
                }
            } else if p.error.is_some() {
                format!("{}: 获取失败", p.alias)
            } else {
                format!("{}: 无数据", p.alias)
            };
            if let Ok(item) = MenuItem::with_id(
                app_handle,
                format!("detail_{}_{}", i, p.alias),
                &text,
                false,
                None::<&str>,
            ) {
                let _ = menu.append(&item); // 供应商展示项失败可忽略
            }
        }

        // 3) 超过上限则追加截断提示
        if summary.providers.len() > MAX_PROVIDER_ITEMS {
            let text = format!("…共 {} 个供应商", summary.providers.len());
            if let Ok(item) =
                MenuItem::with_id(app_handle, "provider_overflow", &text, false, None::<&str>)
            {
                let _ = menu.append(&item);
            }
        }

        // 4) 没有任何服务商时放一个占位项
        if summary.providers.is_empty() {
            if let Ok(item) =
                MenuItem::with_id(app_handle, "no_provider", "暂无服务商", false, None::<&str>)
            {
                let _ = menu.append(&item);
            }
        }

        // 5) 设置 / 退出
        let Ok(separator_bottom) = PredefinedMenuItem::separator(app_handle) else {
            return;
        };
        critical_ok &= menu.append(&separator_bottom).is_ok();
        let Ok(settings_item) =
            MenuItem::with_id(app_handle, "settings", "设置...", true, None::<&str>)
        else {
            return;
        };
        critical_ok &= menu.append(&settings_item).is_ok();
        let Ok(quit_item) = MenuItem::with_id(app_handle, "quit", "退出", true, None::<&str>)
        else {
            return;
        };
        critical_ok &= menu.append(&quit_item).is_ok();

        // 仅当关键项全部装配成功才替换菜单，否则保留旧菜单
        if critical_ok {
            let _ = tray.set_menu(Some(menu));
        }
    }
    let _ = app_handle;
}
