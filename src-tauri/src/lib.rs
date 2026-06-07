use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use tauri::menu::{Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

/// 현재 디렉터리(실패 시 루트). 상대경로를 절대경로로 만들 때 쓴다.
fn current_dir_or_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// 파일 경로로부터 결정적 윈도우 label 생성.
/// 윈도우 label은 `[a-zA-Z0-9-/:_]`만 허용하므로 경로를 직접 쓰지 않고 해시한다.
/// 결정적이므로 같은 경로는 항상 같은 label → `get_webview_window`로 중복을 판별한다.
fn label_for(path: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("win-{}", hasher.finish())
}

/// 해당 label의 윈도우가 살아 있으면 포커스하고 `true`를 반환한다.
fn focus_if_open(app: &AppHandle, label: &str) -> bool {
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.set_focus();
        return true;
    }
    false
}

/// 미존재 경로의 `.`/`..` 를 어휘적으로 정리한다(존재 파일은 canonicalize 사용).
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 경로를 윈도우로 연다. 이미 같은 경로가 열려 있으면 그 윈도우를 포커스하고,
/// 없으면 새 윈도우를 만든다. 존재하지 않는 경로는 빈 창으로 연다(`new=1`).
/// `cwd`는 상대경로를 절대경로로 만들 때 사용한다.
fn open_target(app: &AppHandle, raw: &Path, cwd: &Path) {
    let abs = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    };

    // canonicalize 성공 = 존재(뷰어), 실패 = 미존재(빈 창).
    let (key, exists) = match abs.canonicalize() {
        Ok(c) => (c, true),
        Err(_) => (lexical_normalize(&abs), false),
    };

    // 디렉터리는 열지 않는다(tv 스크립트가 먼저 막지만 이중 안전).
    if exists && key.is_dir() {
        eprintln!("[open] {key:?} is a directory; skipping");
        return;
    }

    // 같은 경로는 같은 label → 이미 열려 있으면 포커스만(요구 1).
    let label = label_for(&key);
    if focus_if_open(app, &label) {
        return;
    }

    let encoded = utf8_percent_encode(&key.to_string_lossy(), NON_ALPHANUMERIC).to_string();
    let title = key
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Text Viewer".to_string());

    let mut url = format!("index.html?path={encoded}");
    if !exists {
        url.push_str("&new=1");
    }

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(900.0, 700.0);

    // 같은 식별자의 파일 창끼리 macOS 네이티브 탭으로 묶는다(⇧⌘\로 탭바 토글).
    #[cfg(target_os = "macos")]
    {
        builder = builder.tabbing_identifier("ft-viewer");
    }

    if let Err(e) = builder.build() {
        eprintln!("[open] failed to create window: {e}");
    }
}

/// 파일 없이 앱을 실행했을 때 보여줄 안내 윈도우(단일).
fn open_welcome_window(app: &AppHandle) {
    if focus_if_open(app, "main") {
        return;
    }
    if let Err(e) = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Multi-Window Text Viewer")
        .inner_size(800.0, 600.0)
        .build()
    {
        eprintln!("[welcome] failed to create window: {e}");
    }
}

/// 설정 윈도우를 연다(단일). 이미 열려 있으면 그 창을 포커스한다.
fn open_settings_window(app: &AppHandle) {
    if focus_if_open(app, "settings") {
        return;
    }
    if let Err(e) = WebviewWindowBuilder::new(
        app,
        "settings",
        WebviewUrl::App("index.html?view=settings".into()),
    )
    .title("설정")
    .inner_size(480.0, 380.0)
    .resizable(false)
    .build()
    {
        eprintln!("[settings] failed to create window: {e}");
    }
}

/// 현재 키 윈도우(활성 창)의 탭바 표시/숨김을 토글한다(macOS).
/// webview 포커스가 아니라 NSApp의 keyWindow를 직접 대상으로 하므로,
/// 창이 활성이기만 하면 동작한다.
#[cfg(target_os = "macos")]
fn toggle_key_window_tab_bar(app: &AppHandle) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    let _ = app.run_on_main_thread(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            let ns_app = NSApplication::sharedApplication(mtm);
            if let Some(win) = ns_app.keyWindow() {
                win.toggleTabBar(None);
            }
        }
    });
}

/// 메뉴를 구성한다: 표준 메뉴(`Menu::default`)에 "보기" 서브메뉴를 더해
/// 설정(⌘,)·탭 바 토글(⇧⌘\)을 네이티브 accelerator로 노출한다.
/// 메뉴 단축키는 창이 활성이면 webview 포커스와 무관하게 동작한다.
fn setup_menu(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::default(app)?;

    let settings_item = MenuItemBuilder::new("설정…")
        .id("settings")
        .accelerator("Cmd+,")
        .build(app)?;
    let toggle_item = MenuItemBuilder::new("탭 바 표시/숨기기")
        .id("toggle_tabbar")
        .accelerator("Shift+Cmd+\\")
        .build(app)?;

    let view = SubmenuBuilder::new(app, "보기")
        .item(&settings_item)
        .item(&toggle_item)
        .build()?;

    menu.append(&view)?;
    app.set_menu(menu)?;
    Ok(())
}

/// CLI 인자(`tv <file>`)를 처리한다. 파일 경로가 있으면 열고(없으면 활성화만),
/// 끝에 앱을 foreground로 가져온다(요구 1·3).
/// 첫 실행은 `setup`에서, 둘째 실행은 single-instance 콜백에서 호출된다.
fn handle_cli(app: &AppHandle, args: Vec<String>, cwd: &Path) {
    // args[0]은 실행 파일 경로. 첫 비옵션 인자를 파일로 본다.
    if let Some(arg) = args.iter().skip(1).find(|a| !a.starts_with('-')) {
        open_target(app, Path::new(arg), cwd);
    }
    #[cfg(target_os = "macos")]
    activate_app(app);
}

/// 앱을 다른 앱들 위로 활성화한다(macOS).
#[cfg(target_os = "macos")]
fn activate_app(app: &AppHandle) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    let _ = app.run_on_main_thread(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            let ns_app = NSApplication::sharedApplication(mtm);
            #[allow(deprecated)]
            ns_app.activateIgnoringOtherApps(true);
        }
    });
}

/// 파일 내용을 읽어 프론트엔드에 전달한다.
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))
}

/// 안내 윈도우의 "파일 열기" 버튼 등에서 호출하는 커맨드.
#[tauri::command]
fn open_file(app: AppHandle, path: String) {
    open_target(&app, Path::new(&path), &current_dir_or_root());
}

/// `tv` CLI 스크립트의 설치 경로(`~/.local/bin/tv`).
fn tv_script_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME 환경변수를 찾을 수 없습니다".to_string())?;
    Ok(PathBuf::from(home).join(".local/bin/tv"))
}

/// 현재 앱 바이너리를 가리키는 `tv` 스크립트 내용을 생성한다.
fn tv_script_body(app_bin: &str) -> String {
    format!(
        r#"#!/bin/sh
# Auto-generated by "Multi-Window Text Viewer". Opens a file in the app.
# Usage: tv [file]   (directories cannot be opened)
APP_BIN="{app_bin}"
if [ ! -x "$APP_BIN" ]; then
  echo "tv: app binary not found at: $APP_BIN" >&2
  exit 1
fi
if [ "$#" -eq 0 ]; then
  nohup "$APP_BIN" >/dev/null 2>&1 &
  exit 0
fi
arg="$1"
if [ -d "$arg" ]; then
  echo "tv: '$arg' is a directory; directories cannot be opened" >&2
  exit 1
fi
nohup "$APP_BIN" "$arg" >/dev/null 2>&1 &
exit 0
"#
    )
}

/// `tv` CLI의 설치 상태.
#[derive(serde::Serialize)]
struct TvStatus {
    /// 설치 경로(`~/.local/bin/tv`).
    path: String,
    /// 스크립트가 존재하는가.
    installed: bool,
    /// 설치 디렉터리가 PATH에 있는가(앱 환경 기준 — best effort).
    on_path: bool,
    /// 스크립트가 현재 앱 바이너리를 가리키는가(앱 위치가 바뀌면 false).
    up_to_date: bool,
}

#[tauri::command]
fn tv_status() -> Result<TvStatus, String> {
    let tv_path = tv_script_path()?;
    let installed = tv_path.is_file();

    let bin_dir = tv_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let on_path = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|p| Path::new(p) == bin_dir);

    let current = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let up_to_date = installed
        && !current.is_empty()
        && std::fs::read_to_string(&tv_path)
            .map(|s| s.contains(&current))
            .unwrap_or(false);

    Ok(TvStatus {
        path: tv_path.to_string_lossy().into_owned(),
        installed,
        on_path,
        up_to_date,
    })
}

/// `tv` 스크립트를 `~/.local/bin/tv`에 설치(또는 갱신)한다.
#[tauri::command]
fn install_tv() -> Result<String, String> {
    let tv_path = tv_script_path()?;
    let dir = tv_path
        .parent()
        .ok_or_else(|| "설치 경로가 올바르지 않습니다".to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("디렉터리 생성 실패: {e}"))?;

    let app_bin = std::env::current_exe().map_err(|e| format!("앱 경로 확인 실패: {e}"))?;
    std::fs::write(&tv_path, tv_script_body(&app_bin.to_string_lossy()))
        .map_err(|e| format!("스크립트 쓰기 실패: {e}"))?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tv_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("실행 권한 설정 실패: {e}"))?;

    Ok(tv_path.to_string_lossy().into_owned())
}

/// `tv` 스크립트를 제거한다.
#[tauri::command]
fn uninstall_tv() -> Result<(), String> {
    let tv_path = tv_script_path()?;
    if tv_path.exists() {
        std::fs::remove_file(&tv_path).map_err(|e| format!("삭제 실패: {e}"))?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    // single-instance는 가장 먼저 등록해야 한다. 둘째 실행의 argv/cwd를
    // 첫 인스턴스로 전달한다(`tv <file>` CLI 진입점).
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            handle_cli(app, argv, Path::new(&cwd));
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            read_text_file,
            open_file,
            tv_status,
            install_tv,
            uninstall_tv
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // 네이티브 메뉴(⌘, / ⇧⌘\)를 구성하고 이벤트를 연결한다.
            setup_menu(&handle)?;
            handle.on_menu_event(|app, event| match event.id().as_ref() {
                "settings" => open_settings_window(app),
                "toggle_tabbar" => {
                    #[cfg(target_os = "macos")]
                    toggle_key_window_tab_bar(app);
                }
                _ => {}
            });
            // 첫 실행의 CLI 인자 처리(`tv <file>`). 둘째 실행은 위 콜백이 받는다.
            handle_cli(&handle, std::env::args().collect(), &current_dir_or_root());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // macOS: Finder에서 "열기"로 더블클릭하면 발생(콜드 스타트·실행 중 모두).
            RunEvent::Opened { urls } => {
                let cwd = current_dir_or_root();
                for url in urls {
                    if let Ok(path) = url.to_file_path() {
                        open_target(app, &path, &cwd);
                    }
                }
            }
            // 시작 직후 열린 윈도우가 하나도 없으면 안내 윈도우를 띄운다.
            RunEvent::Ready => {
                if app.webview_windows().is_empty() {
                    open_welcome_window(app);
                }
            }
            _ => {}
        });
}
