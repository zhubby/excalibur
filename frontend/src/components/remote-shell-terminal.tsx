"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { PlugZap, Square, X } from "lucide-react";
import type { RemoteShellTerminalSession } from "@/components/console-runtime";

type RemoteShellTerminalProps = {
  terminal: RemoteShellTerminalSession;
  busy?: boolean;
  onCloseSession: () => void;
  onDismiss: () => void;
};

type ConnectionState = "connecting" | "connected" | "closed" | "error";

const decoder = new TextDecoder();

function keyToTerminalInput(event: KeyboardEvent<HTMLDivElement>) {
  if (event.ctrlKey && event.key.toLowerCase() === "c") {
    return "\x03";
  }
  if (event.key === "Enter") {
    return "\r";
  }
  if (event.key === "Backspace") {
    return "\x7f";
  }
  if (event.key === "Tab") {
    return "\t";
  }
  const arrows: Record<string, string> = {
    ArrowUp: "\x1b[A",
    ArrowDown: "\x1b[B",
    ArrowRight: "\x1b[C",
    ArrowLeft: "\x1b[D",
  };
  if (event.key in arrows) {
    return arrows[event.key];
  }
  if (!event.metaKey && !event.altKey && event.key.length === 1) {
    return event.key;
  }
  return null;
}

function formatSeconds(seconds: number) {
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return `${minutes}:${String(rest).padStart(2, "0")}`;
}

export function RemoteShellTerminal({
  terminal,
  busy = false,
  onCloseSession,
  onDismiss,
}: RemoteShellTerminalProps) {
  const [connectionState, setConnectionState] = useState<ConnectionState>("connecting");
  const [output, setOutput] = useState("Connecting to remote shell...\n");
  const [now, setNow] = useState(Date.now());
  const socketRef = useRef<WebSocket | null>(null);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const mountedSessionId = terminal.session.id;

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    setConnectionState("connecting");
    setOutput("Connecting to remote shell...\n");
    let disposed = false;
    const socket = new WebSocket(terminal.websocketUrl);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;

    socket.onopen = () => {
      if (disposed) {
        return;
      }
      setConnectionState("connected");
      setOutput((current) => `${current}Connected. Waiting for device agent...\n`);
      viewportRef.current?.focus();
    };
    socket.onmessage = async (event) => {
      if (disposed) {
        return;
      }
      let chunk = "";
      if (typeof event.data === "string") {
        chunk = event.data;
      } else if (event.data instanceof ArrayBuffer) {
        chunk = decoder.decode(event.data);
      } else if (event.data instanceof Blob) {
        chunk = decoder.decode(await event.data.arrayBuffer());
      }
      if (chunk) {
        setOutput((current) => `${current}${chunk}`.slice(-60_000));
      }
    };
    socket.onerror = () => {
      if (disposed) {
        return;
      }
      setConnectionState("error");
      setOutput((current) => `${current}\n[connection error]\n`);
    };
    socket.onclose = () => {
      if (disposed) {
        return;
      }
      setConnectionState((current) => (current === "error" ? "error" : "closed"));
      setOutput((current) => `${current}\n[session closed]\n`);
    };

    return () => {
      disposed = true;
      socket.close();
      if (socketRef.current === socket) {
        socketRef.current = null;
      }
    };
  }, [mountedSessionId, terminal.websocketUrl]);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (viewport) {
      viewport.scrollTop = viewport.scrollHeight;
    }
  }, [output]);

  const expiresIn = useMemo(() => {
    const remaining = Math.max(0, Math.ceil((Date.parse(terminal.session.expires_at) - now) / 1000));
    return formatSeconds(remaining);
  }, [now, terminal.session.expires_at]);

  const sendInput = (value: string) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return;
    }
    socket.send(value);
  };

  const closeSession = () => {
    socketRef.current?.close();
    onCloseSession();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-end bg-black/60 p-3 sm:items-center sm:p-6">
      <section className="panel-in flex max-h-[92vh] w-full flex-col overflow-hidden rounded-md border border-line bg-panel shadow-2xl sm:mx-auto sm:max-w-5xl">
        <div className="flex flex-col gap-3 border-b border-line px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <PlugZap className="h-4 w-4 text-brand" aria-hidden="true" />
              <h2 className="truncate text-base font-semibold text-ink">{terminal.deviceName}</h2>
            </div>
            <p className="mt-1 truncate text-xs text-faint">{terminal.deviceId}</p>
          </div>
          <div className="flex items-center gap-2 text-xs text-muted">
            <span className="rounded-sm bg-elevated px-2 py-1">{connectionState}</span>
            <span className="rounded-sm bg-elevated px-2 py-1">expires {expiresIn}</span>
            <button
              className="inline-flex h-8 items-center justify-center gap-1.5 rounded-md border border-line bg-panel px-2 font-medium text-muted transition hover:bg-line hover:text-ink disabled:cursor-not-allowed disabled:text-faint"
              type="button"
              disabled={busy || connectionState === "closed"}
              onClick={closeSession}
            >
              <Square className="h-3.5 w-3.5" aria-hidden="true" />
              Close
            </button>
            <button
              className="grid h-8 w-8 place-items-center rounded-md text-muted transition hover:bg-line hover:text-ink"
              type="button"
              aria-label="Dismiss terminal"
              onClick={onDismiss}
            >
              <X className="h-4 w-4" aria-hidden="true" />
            </button>
          </div>
        </div>
        <div
          ref={viewportRef}
          className="min-h-[360px] flex-1 overflow-auto bg-rail px-4 py-3 font-mono text-[13px] leading-5 text-ink outline-none sm:min-h-[520px]"
          tabIndex={0}
          role="textbox"
          aria-label="Remote shell terminal input"
          onKeyDown={(event) => {
            const value = keyToTerminalInput(event);
            if (value !== null) {
              event.preventDefault();
              sendInput(value);
            }
          }}
          onPaste={(event) => {
            event.preventDefault();
            sendInput(event.clipboardData.getData("text"));
          }}
        >
          <pre className="whitespace-pre-wrap break-words">{output}</pre>
        </div>
      </section>
    </div>
  );
}
