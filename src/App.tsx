import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

/** 현재 윈도우가 열어야 할 파일 경로 (없으면 안내 모드). */
function pathFromQuery(): string | null {
  return new URLSearchParams(window.location.search).get("path");
}

function Viewer({ path }: { path: string }) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string>("read_text_file", { path })
      .then(setContent)
      .catch((e) => setError(String(e)));
  }, [path]);

  const fileName = path.split("/").pop() ?? path;

  return (
    <main className="viewer">
      <header className="viewer-header">
        <span className="file-name">{fileName}</span>
        <span className="file-path" title={path}>
          {path}
        </span>
      </header>
      {error !== null ? (
        <pre className="error">파일을 열 수 없습니다:{"\n"}{error}</pre>
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
        <code>.ft</code> 파일을 더블클릭하거나 아래 버튼으로 열어보세요.
      </p>
      <button type="button" onClick={pickFile}>
        파일 열기
      </button>
      {error !== null && <p className="error">{error}</p>}
    </main>
  );
}

function App() {
  const path = pathFromQuery();

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

  return path !== null ? <Viewer path={path} /> : <Welcome />;
}

export default App;
