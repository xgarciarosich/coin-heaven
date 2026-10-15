import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { ArrowUp, Eye, Lock, Plus, Settings2, Sparkles, X } from "lucide-react";
import { Banner, Button, Empty } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import type { AiSettings, ChatMessage, ChatScope, EgressPreview, ProviderRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** A turn as the screen holds it: the text, and for an answer what was sent to get it. */
interface Turn extends ChatMessage {
  sent?: EgressPreview;
}

/**
 * Module 5 — the assistant.
 *
 * A conversation about the person's own money and about personal finance in
 * general, and nothing else. It is a chat, so it behaves like one: a
 * question box, the answers above it, a few starting questions when the page
 * is empty.
 *
 * What makes it this app's assistant rather than any chatbot is what it is
 * given and what it is not. Each question travels with a summary of the
 * person's records — totals by month and category, net worth by asset class,
 * the portfolio by holding, the FIRE plan — never a transaction, an account
 * number or a description; anything typed is masked for those before it
 * leaves. The chips above the box decide which summaries go, and every
 * answer can show exactly what was sent to get it.
 *
 * Who answers, and whether to be shown each request first, is decided in
 * Settings. Conversations are held in memory only: closing the page forgets
 * them.
 */
export function ChatView({ onOpenSettings }: { onOpenSettings: () => void }) {
  const t = useT();
  const a = t.assistant;
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [provider, setProvider] = useState<ProviderRow | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const [scope, setScope] = useState<ChatScope>({
    cashflow: true,
    netWorth: true,
    portfolio: true,
    fire: true,
  });
  const [pending, setPending] = useState<EgressPreview | null>(null);
  const [inspect, setInspect] = useState<EgressPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const end = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    Promise.all([api.aiSettingsGet(), api.aiProviderList()])
      .then(([s, p]) => {
        setSettings(s);
        setProvider(p.find((x) => x.isEnabled) ?? null);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, []);

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [turns, busy, pending]);

  const send = useCallback((pv: EgressPreview) => {
    setPending(null);
    setBusy(true);
    api
      .aiChatSend(pv)
      .then((reply) => setTurns((ts) => [...ts, { role: "assistant", content: reply, sent: pv }]))
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => {
        setBusy(false);
        box.current?.focus();
      });
  }, []);

  const ask = (text: string) => {
    const question = text.trim();
    if (!question || busy || pending) return;
    setError(null);
    const next: Turn[] = [...turns, { role: "user", content: question }];
    setTurns(next);
    setDraft("");
    api
      .aiChatPreview(
        next.map(({ role, content }) => ({ role, content })),
        scope,
      )
      .then((pv) => (settings?.requirePreview ? setPending(pv) : send(pv)))
      .catch((e: unknown) => {
        // The question never left: hand it back rather than leave it
        // sitting in the conversation as if it had been asked.
        setTurns(turns);
        setDraft(question);
        setError(msg(e));
      });
  };

  const decline = () => {
    if (!pending) return;
    api.aiDecline(pending).catch(() => undefined);
    const last = turns.at(-1);
    setTurns(turns.slice(0, -1));
    setDraft(last?.role === "user" ? last.content : "");
    setPending(null);
  };

  if (!settings) return null;

  const ready = settings.enabled && provider !== null;
  const chips = (
    [
      ["cashflow", a.scope.cashflow],
      ["netWorth", a.scope.netWorth],
      ["portfolio", a.scope.portfolio],
      ["fire", a.scope.fire],
    ] as const
  ).map(([id, label]) => (
    <button
      key={id}
      type="button"
      aria-pressed={scope[id]}
      onClick={() => setScope((s) => ({ ...s, [id]: !s[id] }))}
      className={cn(
        "rounded-full border px-2.5 py-0.5 text-[11px] transition-colors",
        scope[id]
          ? "border-brass/50 bg-brass-wash text-brass"
          : "border-rule text-ink-3 line-through hover:text-ink",
      )}
    >
      {label}
    </button>
  ));

  return (
    <div className="flex h-full flex-col">
      <header data-tauri-drag-region className="flex items-end justify-between gap-6 px-10 pt-9 pb-5">
        <div>
          <h1 className="display mt-2.5 text-[30px] leading-[1.1] text-ink">{a.title}</h1>
          {ready && (
            <p className="mt-2 flex items-center gap-1.5 text-[12px] text-ink-3">
              {provider.isLocal && <Lock className="size-3 text-positive" strokeWidth={2} />}
              {provider.isLocal
                ? a.answeredLocally(provider.model)
                : a.answeredBy(provider.displayName ?? provider.provider, provider.model)}
            </p>
          )}
        </div>
        <div className="flex shrink-0 gap-2 pb-1">
          {turns.length > 0 && (
            <Button
              onClick={() => {
                setTurns([]);
                setPending(null);
                setError(null);
              }}
            >
              <Plus className="size-3.5" strokeWidth={2} />
              {a.newChat}
            </Button>
          )}
          <Button variant="ghost" onClick={onOpenSettings}>
            <Settings2 className="size-3.5" strokeWidth={1.75} />
            {a.openSettings}
          </Button>
        </div>
      </header>

      {!ready ? (
        <div className="px-10">
          <Empty
            title={settings.enabled ? a.noProvider : a.switchedOff}
            body={a.setupBody}
            action={
              <Button variant="primary" onClick={onOpenSettings}>
                {a.openSettings}
              </Button>
            }
          />
        </div>
      ) : (
        <>
          <div className="min-h-0 flex-1 overflow-y-auto px-10">
            <div className="mx-auto max-w-3xl space-y-5 pb-6">
              {turns.length === 0 && (
                <div className="pt-10">
                  <p className="flex items-center gap-2 text-[13px] text-ink-2">
                    <Sparkles className="size-4 text-brass" strokeWidth={1.75} />
                    {a.intro}
                  </p>
                  <div className="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">
                    {a.starters.map((q) => (
                      <button
                        key={q}
                        type="button"
                        onClick={() => ask(q)}
                        className="rounded-[10px] border border-rule bg-surface px-4 py-3 text-left text-[12.5px] leading-[1.5] text-ink-2 shadow-card transition-colors hover:border-brass hover:text-ink"
                      >
                        {q}
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {turns.map((turn, i) =>
                turn.role === "user" ? (
                  <div key={i} className="flex justify-end">
                    <p
                      data-selectable
                      className="max-w-[80%] rounded-2xl rounded-br-md bg-ink px-4 py-2.5 text-[13px] leading-[1.6] whitespace-pre-wrap text-paper"
                    >
                      {turn.content}
                    </p>
                  </div>
                ) : (
                  <div key={i} className="max-w-[92%]">
                    <div
                      data-selectable
                      className="rounded-2xl rounded-bl-md border border-rule bg-surface px-5 py-4 text-[13px] leading-[1.7] text-ink shadow-card"
                    >
                      <Prose text={turn.content} />
                    </div>
                    {turn.sent && (
                      <button
                        type="button"
                        onClick={() => setInspect(inspect === turn.sent ? null : (turn.sent ?? null))}
                        className="mt-1.5 flex items-center gap-1 pl-1 text-[11px] text-ink-3 transition-colors hover:text-ink"
                      >
                        <Eye className="size-3" strokeWidth={1.75} />
                        {inspect === turn.sent ? a.hideSent : a.showSent}
                      </button>
                    )}
                    {turn.sent && inspect === turn.sent && <Payload preview={turn.sent} />}
                  </div>
                ),
              )}

              {busy && (
                <div className="flex items-center gap-2 pl-1 text-[12px] text-ink-3">
                  <span className="flex gap-1">
                    {[0, 1, 2].map((d) => (
                      <span
                        key={d}
                        className="size-1.5 animate-pulse rounded-full bg-brass"
                        style={{ animationDelay: `${d * 180}ms` }}
                      />
                    ))}
                  </span>
                  {provider.isLocal ? a.thinkingLocal : a.thinking}
                </div>
              )}
              {error && <Banner tone="bad">{error}</Banner>}
              <div ref={end} />
            </div>
          </div>

          <div className="border-t border-rule bg-paper-sunk/60 px-10 pt-3 pb-4">
            <div className="mx-auto max-w-3xl space-y-2.5">
              {pending && (
                <div className="rounded-[10px] border border-brass/50 bg-surface px-4 py-3">
                  <p className="text-[12px] text-ink-2">
                    {a.aboutToSend(pending.bytes, pending.provider)}
                  </p>
                  <Payload preview={pending} />
                  <div className="mt-2.5 flex justify-end gap-2">
                    <Button variant="ghost" onClick={decline}>
                      <X className="size-3.5" strokeWidth={2} />
                      {a.dontSend}
                    </Button>
                    <Button variant="primary" onClick={() => send(pending)}>
                      {a.send}
                    </Button>
                  </div>
                </div>
              )}

              <div className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1 text-[11px] text-ink-3">{a.canSee}</span>
                {chips}
              </div>

              <div className="flex items-end gap-2 rounded-[14px] border border-rule bg-surface px-3 py-2 shadow-card transition-colors focus-within:border-brass">
                <textarea
                  ref={box}
                  rows={1}
                  autoFocus
                  value={draft}
                  placeholder={a.placeholder}
                  onChange={(e) => setDraft(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                      e.preventDefault();
                      ask(draft);
                    }
                  }}
                  className="max-h-40 min-h-[26px] flex-1 resize-none overflow-y-auto border-0 bg-transparent py-1 font-[inherit] text-[13px] leading-[1.5] text-ink shadow-none outline-none placeholder:text-ink-3 focus:outline-none focus-visible:outline-none"
                  // Inline, because the app's global textarea rules (monospace,
                  // a focus ring) are for payload boxes, and the wrapper here
                  // already shows focus.
                  style={{
                    height: `${Math.min(160, 26 + (draft.split("\n").length - 1) * 20)}px`,
                    fontFamily: "inherit",
                    outline: "none",
                  }}
                />
                <button
                  type="button"
                  aria-label={a.send}
                  disabled={!draft.trim() || busy || pending !== null}
                  onClick={() => ask(draft)}
                  className="flex size-7 shrink-0 items-center justify-center rounded-full bg-ink text-paper transition-opacity disabled:opacity-25"
                >
                  <ArrowUp className="size-4" strokeWidth={2} />
                </button>
              </div>
              <p className="text-center text-[10.5px] leading-[1.5] text-ink-3">{a.footnote}</p>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/** Exactly what a request carried. */
function Payload({ preview }: { preview: EgressPreview }) {
  return (
    <pre
      data-selectable
      className="mt-2 max-h-56 overflow-auto rounded-md border border-rule bg-paper-sunk p-3 font-mono text-[11px] leading-[1.5] whitespace-pre-wrap text-ink-2"
    >
      {preview.payload}
    </pre>
  );
}

/**
 * An answer, laid out: paragraphs, lists, headings and bold.
 *
 * Models answer in light Markdown. This reads only that much of it, into
 * elements — the reply is never treated as HTML, so nothing a model writes
 * can become markup.
 */
function Prose({ text }: { text: string }) {
  const blocks: { kind: "p" | "ul" | "ol" | "h"; lines: string[] }[] = [];
  for (const raw of text.replace(/\r/g, "").split("\n")) {
    const line = raw.trimEnd();
    const bullet = /^\s*[-*•]\s+(.*)$/.exec(line);
    const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    const heading = /^\s*#{1,6}\s+(.*)$/.exec(line);
    const last = blocks.at(-1);
    if (line.trim() === "") {
      blocks.push({ kind: "p", lines: [] });
    } else if (heading) {
      blocks.push({ kind: "h", lines: [heading[1] ?? ""] });
      blocks.push({ kind: "p", lines: [] });
    } else if (bullet) {
      if (last?.kind === "ul") last.lines.push(bullet[1] ?? "");
      else blocks.push({ kind: "ul", lines: [bullet[1] ?? ""] });
    } else if (numbered) {
      if (last?.kind === "ol") last.lines.push(numbered[1] ?? "");
      else blocks.push({ kind: "ol", lines: [numbered[1] ?? ""] });
    } else if (last?.kind === "p") {
      last.lines.push(line);
    } else {
      blocks.push({ kind: "p", lines: [line] });
    }
  }
  return (
    <div className="space-y-2.5">
      {blocks
        .filter((b) => b.lines.length > 0)
        .map((b, i) =>
          b.kind === "ul" || b.kind === "ol" ? (
            <ul key={i} className={cn("space-y-1 pl-5", b.kind === "ul" ? "list-disc" : "list-decimal")}>
              {b.lines.map((l, j) => (
                <li key={j}>
                  <Inline text={l} />
                </li>
              ))}
            </ul>
          ) : b.kind === "h" ? (
            <p key={i} className="pt-1 font-medium text-ink">
              <Inline text={b.lines[0] ?? ""} />
            </p>
          ) : (
            <p key={i}>
              {b.lines.map((l, j) => (
                <Fragment key={j}>
                  {j > 0 && " "}
                  <Inline text={l} />
                </Fragment>
              ))}
            </p>
          ),
        )}
    </div>
  );
}

/** `**bold**` and `` `code` `` inside a line; everything else as written. */
function Inline({ text }: { text: string }) {
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("**") && p.endsWith("**") && p.length > 4 ? (
          <strong key={i} className="font-medium text-ink">
            {p.slice(2, -2)}
          </strong>
        ) : p.startsWith("`") && p.endsWith("`") && p.length > 2 ? (
          <code key={i} className="rounded bg-surface-hi px-1 py-0.5 font-mono text-[12px]">
            {p.slice(1, -1)}
          </code>
        ) : (
          <Fragment key={i}>{p}</Fragment>
        ),
      )}
    </>
  );
}
