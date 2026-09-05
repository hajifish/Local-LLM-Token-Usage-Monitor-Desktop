use tauri::{
    AppHandle,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton},
    Manager, Emitter,
};
use crate::models::{ProviderStatus, UsageSummary};

fn tray_icon_path(app: &AppHandle) -> std::path::PathBuf {
    let resource = app.path().resource_dir()
        .expect("failed to get resource dir")
        .join("icons")
        .join("tray-icon@2x.png");
    if resource.exists() {
        resource
    } else {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons").join("tray-icon@2x.png")
    }
}

/// 0 = ok, 1 = yellow (low), 2 = red (empty)
pub fn provider_warning_level(p: &ProviderStatus) -> u8 {
    match &p.balance {
        Some(b) => {
            let v = b.available_balance;
            // 成本型（USD）供应商不参与低余额告警，避免 $0 花费被误判红色
            if b.is_usd() {
                return 0;
            }
            if v <= 0.0 {
                2
            } else if b.is_percent() {
                if v < 5.0 { 1 } else { 0 }
            } else {
                if v < 10.0 { 1 } else { 0 }
            }
        }
        None => 0,
    }
}

pub fn worst_warning(summary: &UsageSummary) -> u8 {
    summary.providers.iter().map(provider_warning_level).max().unwrap_or(0)
}

/// Overlay a colored dot (red/yellow) on the tray icon when any provider is low/empty.
pub fn update_tray_badge(app: &AppHandle, summary: &UsageSummary) {
    let level = worst_warning(summary);
    let Some(tray) = app.tray_by_id("main_tray") else { return };
    let Ok(base) = Image::from_path(tray_icon_path(app)) else { return };
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();

    if level > 0 {
        let (dr, dg, db) = if level == 2 { (255u8, 59u8, 48u8) } else { (255u8, 204u8, 0u8) };
        let cx = w as i32 - 16;
        let cy = 16i32;
        let dot_r = 9i32;
        let ring_r = 12i32;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let dx = x - cx;
                let dy = y - cy;
                let d2 = dx * dx + dy * dy;
                let idx = ((y * w as i32 + x) * 4) as usize;
                if idx + 3 >= rgba.len() { continue; }
                if d2 <= dot_r * dot_r {
                    rgba[idx] = dr; rgba[idx + 1] = dg; rgba[idx + 2] = db; rgba[idx + 3] = 255;
                } else if d2 <= ring_r * ring_r {
                    rgba[idx] = 255; rgba[idx + 1] = 255; rgba[idx + 2] = 255; rgba[idx + 3] = 255;
                }
            }
        }
    }

    let img = Image::new_owned(rgba, w, h);
    let _ = tray.set_icon(Some(img));
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let refresh_item = MenuItem::with_id(app, "refresh", "刷新数据", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "设置...", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    // 初始的占位菜单，scheduler 会动态替换为带详情子菜单的版本
    let menu = Menu::with_items(app, &[
        &refresh_item, &separator, &settings_item, &quit_item,
    ])?;

    // 加载全彩图标（使用 @2x 高分辨率版本，macOS 自动缩放）
    let icon_path = app.path().resource_dir()
        .expect("failed to get resource dir")
        .join("icons")
        .join("tray-icon@2x.png");
    let tray_icon = if icon_path.exists() {
        Image::from_path(&icon_path).unwrap_or_else(|_| app.default_window_icon().unwrap().clone())
    } else {
        // dev 模式下从项目目录加载
        let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons").join("tray-icon@2x.png");
        Image::from_path(&dev_path).unwrap_or_else(|_| app.default_window_icon().unwrap().clone())
    };

    TrayIconBuilder::with_id("main_tray")
        .icon(tray_icon)
        .icon_as_template(false)  // 全彩图标，不启用模板模式
        .menu(&menu)
        .tooltip("LLM Token Monitor")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "quit" => app.exit(0),
                "settings" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                        let _ = window.emit("navigate", "settings");
                    }
                }
                "refresh" => {
                    // manual-refresh 前端无监听者（既有空操作）；直接触发调度器 Notify 立即轮询，
                    // 与 commands.rs save_config 使用同一机制（Notify 已在 lib.rs manage）。
                    if let Some(notify) = app.try_state::<std::sync::Arc<tokio::sync::Notify>>() {
                        notify.notify_one();
                    }
                    let _ = app.emit("manual-refresh", ());
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
