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

function Viewer({ path, isNew }: { path: string | null; isNew: boolean }) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // 새 파일(미존재/제목 없음)은 읽지 않는다. 렌더에서 isNew 분기가 먼저 처리한다.
    if (isNew || path === null) return;
    invoke<string>("read_text_file", { path })
      .then(setContent)
      .catch((e) => setError(String(e)));
  }, [path, isNew]);

  const fileName = path ? path.split("/").pop() ?? path : "제목 없음";

  return (
    <main className="viewer">
      <header className="viewer-header">
        <span className="file-name">{fileName}</span>
        {isNew ? (
          <span className="badge-new">새 파일 · 아직 저장되지 않음</span>
        ) : (
          <span className="file-path" title={path ?? ""}>
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

type TvStatus = {
  path: string;
  installed: boolean;
  on_path: boolean;
  up_to_date: boolean;
};

/** 설정 윈도우(샘플). 표시 설정은 아직 저장되지 않으며, CLI 도구만 실제 동작한다. */
function Settings() {
  const [theme, setTheme] = useState("system");
  const [fontSize, setFontSize] = useState(14);
  const [wrap, setWrap] = useState(true);

  const [tv, setTv] = useState<TvStatus | null>(null);
  const [tvMsg, setTvMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshTv = () =>
    invoke<TvStatus>("tv_status")
      .then(setTv)
      .catch((e) => setTvMsg(String(e)));

  useEffect(() => {
    refreshTv();
  }, []);

  async function run(cmd: "install_tv" | "uninstall_tv", okMsg: (r: string) => string) {
    setBusy(true);
    setTvMsg(null);
    try {
      const r = await invoke<string>(cmd);
      setTvMsg(okMsg(r ?? ""));
      await refreshTv();
    } catch (e) {
      setTvMsg(String(e));
    } finally {
      setBusy(false);
    }
  }

  const tvState = !tv
    ? "확인 중…"
    : !tv.installed
    ? "미설치"
    : tv.up_to_date
    ? "설치됨"
    : "설치됨 (앱 위치가 바뀜 — 재설치 권장)";

  return (
    <main className="settings">
      <h1>설정</h1>
      <p className="settings-note">표시 설정은 샘플입니다(아직 저장되지 않음).</p>

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

      <h2 className="settings-h2">명령줄 도구 (tv)</h2>
      <div className="settings-row">
        <span>상태</span>
        <span>{tvState}</span>
      </div>
      {tv && <p className="settings-note settings-path">{tv.path}</p>}
      {tv?.installed && !tv.on_path && (
        <p className="settings-note">
          터미널에서 <code>tv</code> 가 안 되면 <code>~/.local/bin</code> 을 PATH에
          추가하세요.
        </p>
      )}
      <div className="settings-buttons">
        <button
          type="button"
          disabled={busy}
          onClick={() => run("install_tv", (p) => `설치됨: ${p}`)}
        >
          {tv?.installed ? "재설치" : "설치"}
        </button>
        {tv?.installed && (
          <button
            type="button"
            className="secondary"
            disabled={busy}
            onClick={() => run("uninstall_tv", () => "제거되었습니다.")}
          >
            제거
          </button>
        )}
      </div>
      {tvMsg && <p className="settings-note">{tvMsg}</p>}
    </main>
  );
}

function App() {
  const { view, path, isNew } = queryParams();
  // 단축키(⌘N 새 파일 / ⌘, 설정 / ⇧⌘\ 탭바)는 네이티브 메뉴 accelerator가 처리한다.
  if (view === "settings") return <Settings />;
  // path가 있거나 새 파일(⌘N: path 없이 new=1)이면 뷰어, 그 외엔 안내 창.
  if (path !== null || isNew) return <Viewer path={path} isNew={isNew} />;
  return <Welcome />;
}

export default App;
