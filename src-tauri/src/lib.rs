use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// 열려 있는 파일 윈도우 추적: canonical 절대경로 -> 윈도우 label.
#[derive(Default)]
struct OpenWindows(Mutex<HashMap<PathBuf, String>>);

/// 파일 경로로부터 결정적 윈도우 label 생성.
/// 윈도우 label은 `[a-zA-Z0-9-/:_]`만 허용하므로 경로를 직접 쓰지 않고 해시한다.
fn label_for(path: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("win-{}", hasher.finish())
}

/// 파일을 윈도우로 연다. 이미 같은 파일이 열려 있으면 그 윈도우를 포커스하고,
/// 없으면 새 윈도우를 만든다.
fn open_file_in_window(app: &AppHandle, raw_path: &Path) {
    // 심볼릭 링크/상대경로를 정규화해 같은 파일이 항상 같은 키로 수렴하게 한다.
    let path = match raw_path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[open_file] cannot resolve {raw_path:?}: {e}");
            return;
        }
    };

    let state = app.state::<OpenWindows>();

    // 이미 열린 윈도우가 살아 있으면 포커스만 하고 종료.
    {
        let mut map = state.0.lock().unwrap();
        if let Some(label) = map.get(&path).cloned() {
            if let Some(win) = app.get_webview_window(&label) {
                let _ = win.set_focus();
                return;
            }
            // 닫힌 윈도우의 잔여 항목 → 제거 후 재생성.
            map.remove(&path);
        }
    }

    let label = label_for(&path);
    let encoded = utf8_percent_encode(&path.to_string_lossy(), NON_ALPHANUMERIC).to_string();
    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Text Viewer".to_string());

    let url = WebviewUrl::App(format!("index.html?path={encoded}").into());
    let mut builder = WebviewWindowBuilder::new(app, &label, url)
        .title(title)
        .inner_size(900.0, 700.0);

    // 같은 식별자의 파일 뷰어 창끼리 macOS 네이티브 탭으로 묶는다.
    // 사용자가 타이틀바의 탭을 드래그해 창을 합치거나 분리할 수 있다.
    // (자동 병합은 시스템 설정 "문서를 탭으로 열기: 항상"일 때 동작하며,
    //  그 외에는 Window 메뉴의 "Merge All Windows" 또는 탭 드래그로 합칠 수 있다.)
    #[cfg(target_os = "macos")]
    {
        builder = builder.tabbing_identifier("ft-viewer");
    }

    match builder.build() {
        Ok(win) => {
            state.0.lock().unwrap().insert(path, label.clone());

            // 탭바는 평소 접혀 있고, 사용자가 macOS 표준 단축키
            // "Show/Hide Tab Bar"(⇧⌘\)로 필요할 때만 펼친다.

            // 윈도우가 닫히면 추적 맵에서 제거.
            let app_handle = app.clone();
            win.on_window_event(move |event| {
                if let WindowEvent::Destroyed = event {
                    let state = app_handle.state::<OpenWindows>();
                    state.0.lock().unwrap().retain(|_, v| v != &label);
                }
            });
        }
        Err(e) => eprintln!("[open_file] failed to create window: {e}"),
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

/// 파일 내용을 읽어 프론트엔드에 전달한다.
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))
}

/// 안내 윈도우의 "파일 열기" 버튼 등에서 호출하는 커맨드.
#[tauri::command]
fn open_file(app: AppHandle, path: String) {
    open_file_in_window(&app, Path::new(&path));
}

/// 호출한 창의 탭바 표시/숨김을 토글한다(macOS).
/// 프론트엔드가 ⇧⌘\ 단축키를 잡아 호출한다. 평소엔 접혀 있다가
/// 토글로 펼치면 그 탭을 드래그해 다른 창과 병합/분리할 수 있다.
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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(OpenWindows::default())
        .invoke_handler(tauri::generate_handler![
            read_text_file,
            open_file,
            toggle_tab_bar
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // macOS: 파일을 "열기"로 더블클릭하면 (콜드 스타트·실행 중 모두) 발생.
            RunEvent::Opened { urls } => {
                for url in urls {
                    if let Ok(path) = url.to_file_path() {
                        open_file_in_window(app, &path);
                    }
                }
            }
            // 시작 직후 열린 윈도우가 하나도 없으면 안내 윈도우를 띄운다.
            // (파일로 콜드 스타트한 경우 Opened가 먼저 파일 윈도우를 만들어 여기선 건너뛴다.)
            RunEvent::Ready => {
                if app.webview_windows().is_empty() {
                    open_welcome_window(app);
                }
            }
            _ => {}
        });
}
