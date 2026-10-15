import { useCallback, useEffect, useState } from "react";
import { Check, ShieldAlert, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import {
  Banner,
  Button,
  Checkbox,
  Empty,
  SelectField,
  TextField,
} from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import type { AiSettings, EgressEntry, ProviderRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

const PROVIDERS = [
  { id: "ollama", label: "" },
  { id: "anthropic", label: "Anthropic" },
  { id: "openai", label: "OpenAI" },
  { id: "gemini", label: "Google Gemini" },
  { id: "deepseek", label: "DeepSeek" },
  { id: "qwen", label: "Qwen (Alibaba)" },
  { id: "moonshot", label: "Moonshot (Kimi)" },
  { id: "zai", label: "Z.ai (GLM)" },
  { id: "openai_compatible", label: "" },
];

/**
 * Everything that decides what the AI features may do and where they send
 * it: the master switch, the model, an optional search engine of the
 * person's own, and the record of every request that has ever left.
 *
 * It lives in Settings because it is settings. The assistant is a place to
 * ask questions; the place to decide who answers them is here.
 */
export function AiSettingsPanel() {
  const t = useT();
  const a = t.assistant;
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [providers, setProviders] = useState<ProviderRow[]>([]);
  const [log, setLog] = useState<EgressEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [urlSaved, setUrlSaved] = useState(false);
  const [exaKey, setExaKey] = useState("");
  const [open, setOpen] = useState<number | null>(null);

  const refresh = useCallback(() => {
    Promise.all([api.aiSettingsGet(), api.aiProviderList(), api.aiEgressLog()])
      .then(([s, p, l]) => {
        setSettings(s);
        setUrl(s.searchUrl);
        setProviders(p);
        setLog(l);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, []);

  useEffect(refresh, [refresh]);

  const save = (next: AiSettings) => {
    setSettings(next);
    setError(null);
    return api
      .aiSettingsSave(next)
      .then(refresh)
      .catch((e: unknown) => setError(msg(e)));
  };

  if (!settings) return null;

  return (
    <div className="space-y-5">
      {error && <Banner tone="bad">{error}</Banner>}

      <Panel>
        <PanelHead label={a.privacy} />
        <PanelBody className="space-y-3">
          <Checkbox
            label={a.enable}
            checked={settings.enabled}
            onChange={(e) => save({ ...settings, enabled: e.currentTarget.checked })}
          />
          <Checkbox
            label={a.preview}
            checked={settings.requirePreview}
            onChange={(e) => save({ ...settings, requirePreview: e.currentTarget.checked })}
          />
          <Checkbox
            label={a.raw}
            checked={settings.allowRawDescriptions}
            onChange={(e) => save({ ...settings, allowRawDescriptions: e.currentTarget.checked })}
          />
          <p className="flex items-start gap-2 border-t border-rule pt-3 text-[11.5px] leading-[1.6] text-ink-3">
            <ShieldAlert className="mt-px size-3.5 shrink-0 text-brass" strokeWidth={1.75} />
            <span>{a.privacyNote}</span>
          </p>
        </PanelBody>
      </Panel>

      <ProviderPanel providers={providers} onSaved={refresh} onError={setError} />

      <Panel>
        <PanelHead label={a.searchTitle} />
        <PanelBody className="space-y-3">
          <p className="text-[12px] leading-[1.65] text-ink-2">{a.searchBody}</p>
          <div className="space-y-1.5">
            {(
              [
                ["off", a.searchOff, a.searchOffNote],
                ["duckduckgo", a.searchDdg, a.searchDdgNote],
                ["exa", a.searchExa, a.searchExaNote],
                ["searxng", a.searchSearx, a.searchSearxNote],
              ] as const
            ).map(([id, label, note]) => {
              const on = settings.searchEngine === id;
              return (
                <button
                  key={id}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  onClick={() => !on && save({ ...settings, searchEngine: id })}
                  className={cn(
                    "flex w-full items-start gap-3 rounded-[10px] border px-4 py-3 text-left transition-colors",
                    on ? "border-brass bg-brass-wash/40" : "border-rule hover:border-rule-strong",
                  )}
                >
                  <span
                    className={cn(
                      "mt-[3px] size-3 shrink-0 rounded-full border",
                      on ? "border-brass bg-brass" : "border-rule-strong",
                    )}
                  />
                  <span className="min-w-0">
                    <span className="block text-[13px] text-ink">{label}</span>
                    <span className="mt-0.5 block text-[11.5px] leading-[1.6] text-ink-3">{note}</span>
                  </span>
                </button>
              );
            })}
          </div>
          {settings.searchEngine === "exa" && (
            <div className="space-y-2">
              <div className="flex items-end gap-2">
                <TextField
                  label={a.exaKey}
                  type="password"
                  autoComplete="off"
                  className="w-80"
                  value={exaKey}
                  placeholder={settings.exaKeySet ? "••••••••••••" : a.exaKeyPlaceholder}
                  onChange={(e) => {
                    setExaKey(e.currentTarget.value);
                    setUrlSaved(false);
                  }}
                />
                <Button
                  variant="primary"
                  disabled={exaKey.trim() === ""}
                  onClick={() =>
                    save({ ...settings, exaKey: exaKey.trim() }).then(() => {
                      setExaKey("");
                      setUrlSaved(true);
                    })
                  }
                >
                  {t.common.save}
                </Button>
                {settings.exaKeySet && (
                  <Button variant="ghost" onClick={() => save({ ...settings, exaKey: "" })}>
                    {a.exaKeyRemove}
                  </Button>
                )}
                {urlSaved && (
                  <span className="flex items-center gap-1 pb-2 text-[11.5px] text-positive">
                    <Check className="size-3.5" strokeWidth={2} />
                    {a.searchSaved}
                  </span>
                )}
              </div>
              <p className="text-[11px] leading-[1.6] text-ink-3">
                {settings.exaKeySet ? `${a.exaKeyStored}. ` : ""}
                {a.exaKeyHint}
              </p>
            </div>
          )}
          {settings.searchEngine === "searxng" && (
            <>
              <div className="flex items-end gap-2">
                <TextField
                  label={a.searchUrl}
                  className="w-80"
                  value={url}
                  placeholder="http://localhost:8888"
                  onChange={(e) => {
                    setUrl(e.currentTarget.value);
                    setUrlSaved(false);
                  }}
                />
                <Button
                  variant="primary"
                  disabled={url.trim() === settings.searchUrl}
                  onClick={() =>
                    save({ ...settings, searchUrl: url.trim() }).then(() => setUrlSaved(true))
                  }
                >
                  {t.common.save}
                </Button>
                {urlSaved && (
                  <span className="flex items-center gap-1 pb-2 text-[11.5px] text-positive">
                    <Check className="size-3.5" strokeWidth={2} />
                    {a.searchSaved}
                  </span>
                )}
              </div>
              <p className="text-[11px] leading-[1.6] text-ink-3">{a.searchHint}</p>
            </>
          )}
        </PanelBody>
      </Panel>

      <Panel>
        <PanelHead
          label={a.egress}
          aside={<span className="text-[11px] text-ink-3">{a.egressAside}</span>}
        />
        {log.length === 0 ? (
          <PanelBody>
            <p className="text-[12px] text-ink-3">{a.egressEmpty}</p>
          </PanelBody>
        ) : (
          <div className="max-h-[420px] divide-y divide-rule overflow-auto">
            {log.map((e) => (
              <div key={e.id} className="text-[12px]">
                <button
                  type="button"
                  onClick={() => setOpen(open === e.id ? null : e.id)}
                  className="flex w-full items-center gap-3 px-5 py-2.5 text-left transition-colors hover:bg-surface-hi"
                >
                  <span className="tnum text-ink-3">
                    {e.occurredAt.slice(0, 16).replace("T", " ")}
                  </span>
                  <span className="text-ink">{e.provider}</span>
                  <span className="text-ink-3">{a.purposes[e.purpose] ?? e.purpose}</span>
                  <span
                    className={cn(
                      "ml-auto text-[11px]",
                      e.outcome === "sent" && "text-ink-2",
                      e.outcome === "failed" && "text-negative",
                      e.outcome === "declined" && "text-brass",
                    )}
                  >
                    {a.outcomes[e.outcome] ?? e.outcome}
                    {e.requestBytes != null && ` · ${e.requestBytes}B`}
                  </span>
                </button>
                {open === e.id && (
                  <pre
                    data-selectable
                    className="mx-5 mb-3 max-h-64 overflow-auto rounded-md border border-rule bg-paper-sunk p-3 font-mono text-[11px] leading-[1.5] whitespace-pre-wrap text-ink-2"
                  >
                    {e.payloadPreview}
                  </pre>
                )}
              </div>
            ))}
          </div>
        )}
      </Panel>
    </div>
  );
}

function ProviderPanel({
  providers,
  onSaved,
  onError,
}: {
  providers: ProviderRow[];
  onSaved: () => void;
  onError: (m: string) => void;
}) {
  const a = useT().assistant;
  const label = (p: { id: string; label: string }) =>
    p.id === "openai_compatible" ? a.otherCompatible : p.id === "ollama" ? a.ollama : p.label;
  // Local first: it is the only choice where nothing leaves the machine.
  const [provider, setProvider] = useState("ollama");
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [key, setKey] = useState("");

  const known = PROVIDERS.find((p) => p.id === provider);

  return (
    <Panel>
      <PanelHead label={a.providers} />
      {providers.length > 0 && (
        <div className="divide-y divide-rule">
          {providers.map((p) => (
            <div key={p.id} className="flex items-center gap-3 px-5 py-2.5 text-[12.5px]">
              <span className="w-40 text-ink">{p.provider}</span>
              <span className="flex-1 font-mono text-[11.5px] text-ink-3">{p.model}</span>
              {p.isLocal && (
                <span className="rounded bg-positive/15 px-1.5 py-0.5 text-[10px] text-positive">
                  {a.local}
                </span>
              )}
              {p.requiresKey && (
                <span
                  className={cn(
                    "text-[11px]",
                    p.hasKey ? "text-ink-3" : "text-negative",
                  )}
                >
                  {p.hasKey ? a.keyStored : a.noKey}
                </span>
              )}
              <span className={cn("text-[11px]", p.isEnabled ? "text-positive" : "text-ink-3")}>
                {p.isEnabled ? a.enabled : a.off}
              </span>
              <Button
                variant="ghost"
                onClick={() =>
                  api
                    .aiProviderSave({ ...p, isEnabled: !p.isEnabled }, null)
                    .then(onSaved)
                    .catch((e: unknown) => onError(msg(e)))
                }
              >
                {p.isEnabled ? a.disable : a.enableBtn}
              </Button>
              <Button
                variant="ghost"
                onClick={() =>
                  api
                    .aiProviderDelete(p.provider)
                    .then(onSaved)
                    .catch((e: unknown) => onError(msg(e)))
                }
              >
                <X className="size-3" strokeWidth={2} />
              </Button>
            </div>
          ))}
        </div>
      )}
      <PanelBody className="space-y-3">
        {providers.length === 0 && (
          <Empty
            title={a.noProvider}
            body={a.noProviderBody}
          />
        )}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <SelectField
            label={a.provider}
            value={provider}
            onChange={(e) => setProvider(e.currentTarget.value)}
          >
            {PROVIDERS.map((p) => (
              <option key={p.id} value={p.id}>
                {label(p)}
              </option>
            ))}
          </SelectField>
          <TextField
            label={a.model}
            value={model}
            placeholder={a.modelHint}
            onChange={(e) => setModel(e.currentTarget.value)}
          />
          <TextField
            label={a.baseUrl}
            value={baseUrl}
            placeholder={a.baseUrlDefault}
            onChange={(e) => setBaseUrl(e.currentTarget.value)}
          />
          <TextField
            label={a.apiKey}
            type="password"
            value={key}
            placeholder={provider === "ollama" ? a.notNeeded : "sk-…"}
            disabled={provider === "ollama"}
            onChange={(e) => setKey(e.currentTarget.value)}
          />
        </div>
        <div className="flex items-center justify-between">
          <p className="text-[11px] text-ink-3">
            {a.keysNote}
          </p>
          <Button
            variant="primary"
            disabled={!model.trim()}
            onClick={() =>
              api
                .aiProviderSave(
                  {
                    provider,
                    displayName: known ? label(known) : null,
                    model,
                    baseUrl: baseUrl || null,
                    isEnabled: true,
                    maxOutputTokens: 2000,
                    temperature: 0,
                    systemPrompt: null,
                  },
                  key || null,
                )
                .then(() => {
                  setModel("");
                  setKey("");
                  setBaseUrl("");
                  onSaved();
                })
                .catch((e: unknown) => onError(msg(e)))
            }
          >
            {a.saveProvider}
          </Button>
        </div>
      </PanelBody>
    </Panel>
  );
}
