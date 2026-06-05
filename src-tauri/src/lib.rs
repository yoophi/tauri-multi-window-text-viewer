use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// 열려 있는 윈도우 추적: 절대경로(존재 시 canonical) -> 윈도우 label.
#[derive(Default)]
struct OpenWindows(Mutex<HashMap<PathBuf, String>>);

/// 파일 경로로부터 결정적 윈도우 label 생성.
/// 윈도우 label은 `[a-zA-Z0-9-/:_]`만 허용하므로 경로를 직접 쓰지 않고 해시한다.
fn label_for(path: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("win-{}", hasher.finish())
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

    let state = app.state::<OpenWindows>();

    // 이미 열린 윈도우가 살아 있으면 포커스만 하고 종료(요구 1).
    {
        let mut map = state.0.lock().unwrap();
        if let Some(label) = map.get(&key).cloned() {
            if let Some(win) = app.get_webview_window(&label) {
                let _ = win.set_focus();
                return;
            }
            map.remove(&key);
        }
    }

    let label = label_for(&key);
    let encoded = utf8_percent_encode(&key.to_string_lossy(), NON_ALPHANUMERIC).to_string();
    let title = key
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Text Viewer".to_string());

    let mut query = format!("index.html?path={encoded}");
    if !exists {
        query.push_str("&new=1");
    }

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(query.into()))
        .title(title)
        .inner_size(900.0, 700.0);

    // 같은 식별자의 파일 창끼리 macOS 네이티브 탭으로 묶는다(⇧⌘\로 탭바 토글).
    #[cfg(target_os = "macos")]
    {
        builder = builder.tabbing_identifier("ft-viewer");
    }

    match builder.build() {
        Ok(win) => {
            state.0.lock().unwrap().insert(key, label.clone());

            // 윈도우가 닫히면 추적 맵에서 제거.
            let app_handle = app.clone();
            win.on_window_event(move |event| {
                if let WindowEvent::Destroyed = event {
                    let state = app_handle.state::<OpenWindows>();
                    state.0.lock().unwrap().retain(|_, v| v != &label);
                }
            });
        }
        Err(e) => eprintln!("[open] failed to create window: {e}"),
    }
}

/// 파일 없이 앱을 실행했을 때 보여줄 안내 윈도우.
fn open_welcome_window(app: &AppHandle) {
    if app.get_webview_window("main").is_some() {
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
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    open_target(&app, Path::new(&path), &cwd);
}

/// 호출한 창의 탭바 표시/숨김을 토글한다(macOS). 프론트엔드가 ⇧⌘\ 로 호출한다.
#[tauri::command]
fn toggle_tab_bar(window: tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWindow;
        let _ = window.clone().run_on_main_thread(move || unsafe {
            if let Ok(ptr) = window.ns_window() {
                let ns_window: &NSWindow = &*ptr.cast::<NSWindow>();
                ns_window.toggleTabBar(None);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
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
        .manage(OpenWindows::default())
        .invoke_handler(tauri::generate_handler![
            read_text_file,
            open_file,
            toggle_tab_bar
        ])
        .setup(|app| {
            // 첫 실행의 CLI 인자 처리(`tv <file>`). 둘째 실행은 위 콜백이 받는다.
            let args: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            handle_cli(app.handle(), args, &cwd);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // macOS: Finder에서 "열기"로 더블클릭하면 발생(콜드 스타트·실행 중 모두).
            RunEvent::Opened { urls } => {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
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
