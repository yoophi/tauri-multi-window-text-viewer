import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

/** 현재 윈도우가 표시할 내용: 뷰(설정 등)·경로·"새 파일"(미존재) 여부. */
function queryParams(): {
  view: string | null;
  path: string | null;
  isNew: boolean;
} {
  const p = new URLSearchParams(window.location.search);
  return {
    view: p.get("view"),
    path: p.get("path"),
    isNew: p.get("new") === "1",
  };
}

function Viewer({ path, isNew }: { path: string; isNew: boolean }) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // 새 파일(미존재)은 읽지 않는다. 렌더에서 isNew 분기가 먼저 처리한다.
    if (isNew) return;
    invoke<string>("read_text_file", { path })
      .then(setContent)
      .catch((e) => setError(String(e)));
  }, [path, isNew]);

  const fileName = path.split("/").pop() ?? path;

  return (
    <main className="viewer">
      <header className="viewer-header">
        <span className="file-name">{fileName}</span>
        {isNew ? (
          <span className="badge-new">새 파일 · 아직 저장되지 않음</span>
        ) : (
          <span className="file-path" title={path}>
            {path}
          </span>
        )}
      </header>
      {error !== null ? (
        <pre className="error">파일을 열 수 없습니다:{"\n"}{error}</pre>
      ) : isNew ? (
        <p className="empty-hint">빈 파일입니다. (편집·저장 기능은 준비 중)</p>
      ) : content === null ? (
        <p className="loading">불러오는 중…</p>
      ) : (
        <pre className="content">{content}</pre>
      )}
    </main>
  );
}

function Welcome() {
  const [error, setError] = useState<string | null>(null);

  async function pickFile() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "FT Text File", extensions: ["ft"] }],
      });
      if (typeof selected === "string") {
        await invoke("open_file", { path: selected });
      }
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <main className="welcome">
      <h1>Multi-Window Text Viewer</h1>
      <p className="hint">
        <code>.ft</code> 파일을 더블클릭하거나, 터미널에서 <code>tv &lt;파일&gt;</code>,
        또는 아래 버튼으로 열어보세요.
      </p>
      <button type="button" onClick={pickFile}>
        파일 열기
      </button>
      {error !== null && <p className="error">{error}</p>}
    </main>
  );
}

/** 설정 윈도우(샘플). 실제 저장 없이 UI 형태만 보여준다. */
function Settings() {
  const [theme, setTheme] = useState("system");
  const [fontSize, setFontSize] = useState(14);
  const [wrap, setWrap] = useState(true);

  return (
    <main className="settings">
      <h1>설정</h1>
      <p className="settings-note">샘플 화면입니다. 값은 아직 저장되지 않습니다.</p>

      <label className="settings-row">
        <span>테마</span>
        <select value={theme} onChange={(e) => setTheme(e.target.value)}>
          <option value="system">시스템</option>
          <option value="light">라이트</option>
          <option value="dark">다크</option>
        </select>
      </label>

      <label className="settings-row">
        <span>글꼴 크기</span>
        <span className="settings-control">
          <input
            type="range"
            min={10}
            max={24}
            value={fontSize}
            onChange={(e) => setFontSize(Number(e.target.value))}
          />
          <span className="settings-value">{fontSize}px</span>
        </span>
      </label>

      <label className="settings-row">
        <span>자동 줄바꿈</span>
        <input
          type="checkbox"
          checked={wrap}
          onChange={(e) => setWrap(e.target.checked)}
        />
      </label>
    </main>
  );
}

function App() {
  const { view, path, isNew } = queryParams();

  // 전역 단축키: ⇧⌘\ 탭바 토글, ⌘, 설정 창.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.metaKey && e.shiftKey && e.code === "Backslash") {
        e.preventDefault();
        invoke("toggle_tab_bar").catch(() => {});
      } else if (e.metaKey && e.key === ",") {
        e.preventDefault();
        invoke("open_settings").catch(() => {});
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  if (view === "settings") return <Settings />;
  return path !== null ? <Viewer path={path} isNew={isNew} /> : <Welcome />;
}

export default App;
