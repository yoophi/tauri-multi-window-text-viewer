import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

/** 현재 윈도우가 열어야 할 경로와 "새 파일"(미존재) 여부. */
function queryParams(): { path: string | null; isNew: boolean } {
  const p = new URLSearchParams(window.location.search);
  return { path: p.get("path"), isNew: p.get("new") === "1" };
}

function Viewer({ path, isNew }: { path: string; isNew: boolean }) {
  // 새 파일(미존재)은 읽지 않고 빈 내용으로 시작한다.
  const [content, setContent] = useState<string | null>(isNew ? "" : null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
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

function App() {
  const { path, isNew } = queryParams();

  // ⇧⌘\ 로 이 창의 탭바를 토글한다(공간 절약: 평소 숨김, 필요할 때만 펼침).
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.metaKey && e.shiftKey && e.code === "Backslash") {
        e.preventDefault();
        invoke("toggle_tab_bar").catch(() => {});
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return path !== null ? <Viewer path={path} isNew={isNew} /> : <Welcome />;
}

export default App;
