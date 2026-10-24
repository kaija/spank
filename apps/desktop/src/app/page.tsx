"use client";

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type AppInfo = { version: string; defaultConfig: unknown };

export default function SettingsPage() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<AppInfo>("app_info").then(setInfo, (e) => setError(String(e)));
  }, []);

  return (
    <main>
      <h1>Spank 設定</h1>
      <p>Scaffold（T08）：真正的設定頁面會依 Claude Design 的設計實作。</p>
      {error && <p role="alert">無法連線到 Rust core：{error}</p>}
      {info && (
        <>
          <p>版本 {info.version}</p>
          <h2>預設設定（來自 spank-core）</h2>
          <pre>{JSON.stringify(info.defaultConfig, null, 2)}</pre>
        </>
      )}
    </main>
  );
}
