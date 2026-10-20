/**
 * The only place the UI talks to the backend.
 *
 * There is no `fetch` anywhere in this codebase: the CSP blocks outbound
 * connections from the webview, and every privileged operation — including any
 * future AI call — goes through a named Rust command.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  AppErrorPayload,
  AppStatus,
  Balances,
  BackupOutcome,
  BackupStatus,
  Currency,
  FireParams,
  FireResult,
  SimParams,
  SimResult,
  Account,
  AssetClass,
  AssetKind,
  FirePlanView,
  LedgerSuggestion,
  HistoryPoint,
  HoldingTxn,
  PortfolioView,
  PriceInput,
  PriceProposal,
  Schedule,
  PlanParams,
  AiSettings,
  AppSettings,
  CategoryUsage,
  Batch,
  EgressEntry,
  EgressPreview,
  ProviderConfig,
  ProviderRow,
  ApplyOutcome,
  ChatMessage,
  ChatScope,
  MerchantSuggestion,
  Assignment,
  BudgetPlan,
  ExportOutcome,
  ImportImpact,
  MarketAsk,
  MarketResult,
  MonthPlan,
  RecurringReport,
  SampleStatus,
  UpdateInfo,
  PlanItem,
  PlanSetup,
  BudgetSeed,
  CashflowSummary,
  Matrix,
  Suggestions,
  Category,
  DateFormat,
  EventInput,
  PeriodTotals,
  TxnEdit,
  FireView,
  Snapshot,
  SnapshotInput,
  SnapshotPrompt,
  EventItem,
  Ledger,
  PlSummary,
  TxnFilter,
  BankMapping,
  ImportSummary,
  Inspection,
  PassphraseStrength,
  PendingSplit,
  PendingTransfer,
  Preview,
  Rule,
  StoredMapping,
  UnlockState,
  UnlockStatus,
} from "./types";

export class IpcError extends Error {
  constructor(readonly payload: AppErrorPayload) {
    super(payload.message);
    this.name = "IpcError";
  }
}

/** True when running under `pnpm dev` in a plain browser, with no Rust behind. */
const standalone =
  import.meta.env.DEV &&
  typeof window !== "undefined" &&
  !("__TAURI_INTERNALS__" in window);

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (standalone) {
    const { mockInvoke } = await import("@fixtures");
    return mockInvoke<T>(cmd, args);
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "kind" in e && "message" in e) {
      throw new IpcError(e as AppErrorPayload);
    }
    throw e;
  }
}

export const api = {
  appStatus: () => call<AppStatus>("app_status"),
  simDefaults: () => call<SimParams>("sim_defaults"),
  simRun: (params: SimParams) => call<SimResult>("sim_run", { params }),
  simListScenarios: () =>
    call<{ id: number; name: string; isDefault: boolean }[]>(
      "sim_list_scenarios",
    ),
  simSaveScenario: (name: string, params: SimParams) =>
    call<number>("sim_save_scenario", { name, params }),
  simLoadScenario: (id: number) => call<SimParams>("sim_load_scenario", { id }),
  piiPreview: (text: string) =>
    call<{ text: string; redactions: Record<string, number> }>("pii_preview", {
      text,
    }),

  // Recovery. `recoveryTakePhrase` returns the phrase exactly once — the
  // backend clears it on read, so the UI must not drop it before the user has
  // written it down.
  unlockStatus: () => call<UnlockStatus>("unlock_status"),

  /** First run. `null` is an explicit "no passphrase", not a default. */
  setupDatabase: (passphrase: string | null) =>
    call<UnlockState>("setup_database", { passphrase }),
  passphraseStrength: (passphrase: string) =>
    call<PassphraseStrength>("passphrase_strength", { passphrase }),

  recoveryTakePhrase: () => call<string | null>("recovery_take_phrase"),
  recoveryConfirmPhrase: (phrase: string) =>
    call<boolean>("recovery_confirm_phrase", { phrase }),
  recoveryDeferConfirmation: () => call<void>("recovery_defer_confirmation"),

  unlockWithPassphrase: (passphrase: string) =>
    call<UnlockState>("unlock_with_passphrase", { passphrase }),
  unlockWithRecoveryPhrase: (phrase: string) =>
    call<UnlockState>("unlock_with_recovery_phrase", { phrase }),
  upgradeLegacyDatabase: (phrase: string) =>
    call<UnlockState>("upgrade_legacy_database", { phrase }),

  passphraseSet: (passphrase: string) =>
    call<void>("passphrase_set", { passphrase }),
  passphraseRemove: () => call<void>("passphrase_remove"),

  // Backup
  backupStatus: () => call<BackupStatus>("backup_status"),
  backupSetDirectory: (path: string) =>
    call<void>("backup_set_directory", { path }),
  backupNow: () => call<BackupOutcome>("backup_now"),

  // FIRE
  fireDefaults: () => call<FireParams>("fire_defaults"),
  fireCalculate: (params: FireParams, balances: Balances) =>
    call<FireResult>("fire_calculate", { params, balances }),
  fireLatestBalances: () => call<Balances | null>("fire_latest_balances"),

  // Pending matches — proposals only; nothing is applied without a decision.
  // --- Module 1: import ---
  accountList: () => call<Account[]>("account_list"),
  accountCreate: (
    name: string,
    kind: string,
    currency: string,
    institution: string | null,
  ) => call<number>("account_create", { name, kind, currency, institution }),

  mappingList: () => call<StoredMapping[]>("mapping_list"),
  mappingSave: (id: number | null, mapping: BankMapping) =>
    call<number>("mapping_save", { id, mapping }),
  mappingDelete: (id: number) => call<void>("mapping_delete", { id }),

  /** The formats the importer understands, for the picker. */
  dateFormats: () => call<DateFormat[]>("date_formats"),
  /** Open a file and work out its format. Local, deterministic, no egress. */
  importInspect: (path: string) => call<Inspection>("import_inspect", { path }),
  importPreview: (path: string, mapping: BankMapping) =>
    call<Preview>("import_preview", { path, mapping }),
  importRun: (
    accountId: number,
    mappingId: number | null,
    path: string,
    mapping: BankMapping,
  ) => call<ImportSummary>("import_run", { accountId, mappingId, path, mapping }),
  importBatches: () => call<Batch[]>("import_batches"),
  importImpact: (batchId: number) => call<ImportImpact>("import_impact", { batchId }),
  /** `everything` deletes every transaction of the import, worked-on ones too. */
  importUndo: (batchId: number, everything = false) =>
    call<number>("import_undo", { batchId, everything }),

  ruleList: () => call<Rule[]>("rule_list"),
  ruleSave: (rule: Rule) => call<number>("rule_save", { rule }),
  ruleDelete: (id: number) => call<void>("rule_delete", { id }),
  /** Run the rules across everything still awaiting review. */
  rulesApply: () => call<number>("rules_apply"),

  matchingPendingTransfers: () =>
    call<PendingTransfer[]>("matching_pending_transfers"),
  matchingPendingSplits: () => call<PendingSplit[]>("matching_pending_splits"),
  matchingDecide: (
    kind: "transfer" | "split",
    txnA: number,
    txnB: number,
    confirm: boolean,
  ) => call<void>("matching_decide", { kind, txnA, txnB, confirm }),

  currencyList: () => call<Currency[]>("currency_list"),

  // --- global settings ---
  /** Hands the active region's merchant boilerplate words to the importer. */
  regionApply: (merchantNoise: string[]) => call<void>("region_apply", { merchantNoise }),
  settingsGet: () => call<AppSettings>("settings_get"),
  settingsSave: (settings: AppSettings) =>
    call<void>("settings_save", { settings }),
  categoryUsage: (id: number) => call<CategoryUsage>("category_usage", { id }),
  /** `reassignTo` is required when the category is still in use. */
  categoryDelete: (id: number, reassignTo: number | null) =>
    call<void>("category_delete", { id, reassignTo }),
  categoryReorder: (orderedIds: number[]) =>
    call<void>("category_reorder", { orderedIds }),

  // --- Module 2: categories, events, ledger ---
  categoryList: () => call<Category[]>("category_list"),
  categorySave: (
    id: number | null,
    name: string,
    kind: string,
    color: string | null,
  ) => call<number>("category_save", { id, name, kind, color }),
  categoryArchive: (id: number) => call<void>("category_archive", { id }),

  eventList: () => call<EventItem[]>("event_list"),
  eventSave: (id: number | null, input: EventInput) =>
    call<number>("event_save", { id, input }),
  eventSetClosed: (id: number, closed: boolean) =>
    call<void>("event_set_closed", { id, closed }),
  eventDelete: (id: number) => call<void>("event_delete", { id }),

  txnList: (filter: TxnFilter) => call<Ledger>("txn_list", { filter }),
  /** `null` leaves a field alone; clearing is explicit. */
  txnAssign: (
    ids: number[],
    categoryId: number | null,
    projectId: number | null,
    clearCategory?: boolean,
    clearProject?: boolean,
  ) =>
    call<number>("txn_assign", {
      ids,
      categoryId,
      projectId,
      clearCategory: clearCategory ?? false,
      clearProject: clearProject ?? false,
    }),
  txnSetNeutral: (ids: number[], neutral: boolean) =>
    call<void>("txn_set_neutral", { ids, neutral }),
  txnLinkReimbursement: (parentId: number | null, childId: number) =>
    call<void>("txn_link_reimbursement", { parentId, childId }),

  plSummary: (from: string, to: string) =>
    call<PlSummary>("pl_summary", { from, to }),

  /** The amount is not editable: it came from a statement. */
  txnUpdate: (id: number, edit: TxnEdit) => call<void>("txn_update", { id, edit }),
  /** For good: a deleted row does not come back when its statement is re-imported. */
  txnDelete: (ids: number[]) => call<number>("txn_delete", { ids }),
  /** Years derived from the data, so a new year appears with no code change. */
  txnYears: () => call<number[]>("txn_years"),
  cashflowMonthly: (from: string, to: string) =>
    call<PeriodTotals[]>("cashflow_monthly", { from, to }),
  cashflowYearly: () => call<PeriodTotals[]>("cashflow_yearly"),
  cashflowSummary: (from: string, to: string) =>
    call<CashflowSummary>("cashflow_summary", { from, to }),

  /** Every category's net, month by month, for one year. */
  plMatrix: (year: number) => call<Matrix>("pl_matrix", { year }),

  /** Every budgeted category of a year: the year, and its twelve months. */
  /** Every month that has a monthly plan, oldest first. */
  planList: () => call<MonthPlan[]>("plan_list"),
  /** Holdings with buys known only by their amount, and what would be asked. */
  marketPreview: () => call<MarketAsk[]>("market_preview"),
  /** Asks the market-data source for the closes on those buy dates. */
  marketFetch: (holdingIds: number[]) => call<MarketResult[]>("market_fetch", { holdingIds }),
  marketClear: (holdingId: number) => call<number>("market_clear", { holdingId }),
  /** Payments that come round on a rhythm, found in the ledger. */
  recurringList: () => call<RecurringReport>("recurring_list"),
  /** Writes CSV files into a new folder inside `folder`. */
  dataExport: (folder: string, delimiter: string, decimal: string) =>
    call<ExportOutcome>("data_export", { folder, delimiter, decimal }),
  sampleStatus: () => call<SampleStatus>("sample_status"),
  sampleLoad: (language: string, currency: string) =>
    call<void>("sample_load", { language, currency }),
  sampleRemove: () => call<void>("sample_remove"),
  /** One request to the project's public release page. Opt-in. */
  updateCheck: () => call<UpdateInfo>("update_check"),
  /** Replaces the month's boxes; the array's order is the order kept. */
  planSave: (period: string, items: PlanItem[], setup?: PlanSetup) =>
    call<void>("plan_save", { period, items, setup: setup ?? null }),
  planDelete: (period: string) => call<void>("plan_delete", { period }),

  budgetPlan: (year: number) => call<BudgetPlan[]>("budget_plan", { year }),
  /** `period` is a year ("2026") or a month ("2026-03"). */
  budgetSave: (categoryId: number, period: string, amountMinor: number) =>
    call<void>("budget_save", { categoryId, period, amountMinor }),
  budgetSaveEveryMonth: (categoryId: number, year: number, amountMinor: number) =>
    call<void>("budget_save_every_month", { categoryId, year, amountMinor }),
  /** A month goes back to following the year; a year takes its months with it. */
  budgetRemove: (categoryId: number, period: string) =>
    call<void>("budget_remove", { categoryId, period }),
  budgetSeedFromYear: (sourceYear: number, targetYear: number) =>
    call<BudgetSeed[]>("budget_seed_from_year", { sourceYear, targetYear }),

  /** Group everything uncategorised by merchant and propose a category. */
  autocatSuggest: () => call<Suggestions>("autocat_suggest"),
  autocatApply: (assignments: Assignment[]) =>
    call<ApplyOutcome>("autocat_apply", { assignments }),

  // --- Module 3: net worth and FIRE ---
  snapshotList: () => call<Snapshot[]>("snapshot_list"),
  snapshotSave: (id: number | null, input: SnapshotInput) =>
    call<number>("snapshot_save", { id, input }),
  snapshotDelete: (id: number) => call<void>("snapshot_delete", { id }),
  /** Rename an item on every date it appears. */
  wealthLineRename: (bucket: string, from: string, to: string) =>
    call<number>("wealth_line_rename", { bucket, from, to }),
  snapshotPrompt: () => call<SnapshotPrompt>("snapshot_prompt"),
  /** The order items are shown in under each asset class, by name. */
  wealthItemOrder: () => call<Record<string, string[]>>("wealth_item_order"),
  wealthItemReorder: (bucket: string, labels: string[]) =>
    call<void>("wealth_item_reorder", { bucket, labels }),
  holdingReorder: (orderedIds: number[]) => call<void>("holding_reorder", { orderedIds }),
  fireView: (params: FireParams | null) =>
    call<FireView>("fire_view", { params }),

  /** Where you are, and when you arrive. */
  firePlan: (params: FireParams | null, planParams: PlanParams | null) =>
    call<FirePlanView>("fire_plan", { params, planParams }),
  fireParamsSave: (params: FireParams, planParams: PlanParams) =>
    call<void>("fire_params_save", { params, planParams }),
  portfolioView: (asOf: string | null) => call<PortfolioView>("portfolio_view", { asOf }),
  /** Share of the portfolio that is gain, in bps, for the FIRE tax estimate. */
  portfolioGainShare: () => call<number | null>("portfolio_gain_share"),
  holdingSave: (id: number | null, name: string, symbol: string | null, assetClass: string) =>
    call<number>("holding_save", { id, name, symbol, assetClass }),
  holdingDelete: (id: number) => call<void>("holding_delete", { id }),
  holdingTxns: (holdingId: number) => call<HoldingTxn[]>("holding_txns", { holdingId }),
  holdingTxnSave: (
    id: number | null,
    holdingId: number,
    onDate: string,
    kind: HoldingTxn["kind"],
    amountMinor: number,
    unitsE8: number | null,
  ) => call<number>("holding_txn_save", { id, holdingId, onDate, kind, amountMinor, unitsE8 }),
  holdingTxnDelete: (id: number) => call<void>("holding_txn_delete", { id }),
  scheduleSave: (schedule: Schedule) => call<number>("schedule_save", { schedule }),
  scheduleDelete: (id: number) => call<void>("schedule_delete", { id }),
  /** Record prices; every recorded balance is revalued. Returns how many were saved. */
  pricesSave: (prices: PriceInput[]) => call<number>("prices_save", { prices }),
  /** Value and money put in, month by month. */
  portfolioHistory: () => call<HistoryPoint[]>("portfolio_history_view"),
  aiPricePreview: () => call<EgressPreview>("ai_price_preview"),
  aiPriceSend: (preview: EgressPreview) => call<PriceProposal[]>("ai_price_send", { preview }),

  /** Last twelve complete months of spending and saving, from the ledger. */
  fireLedgerSuggestion: () => call<LedgerSuggestion>("fire_ledger_suggestion"),

  assetClassList: () => call<AssetClass[]>("asset_class_list"),
  assetClassSave: (
    key: string | null,
    name: string,
    kind: AssetKind,
    color: string | null,
    countsFire: boolean,
  ) => call<string>("asset_class_save", { key, name, kind, color, countsFire }),
  assetClassDelete: (key: string, reassignTo: string | null) =>
    call<void>("asset_class_delete", { key, reassignTo }),
  assetClassReorder: (orderedKeys: string[]) =>
    call<void>("asset_class_reorder", { orderedKeys }),

  // --- Module 5: assistant ---
  aiSettingsGet: () => call<AiSettings>("ai_settings_get"),
  aiSettingsSave: (settings: AiSettings) =>
    call<void>("ai_settings_save", { settings }),

  aiProviderList: () => call<ProviderRow[]>("ai_provider_list"),
  /** `apiKey: null` leaves any stored key untouched. */
  aiProviderSave: (config: ProviderConfig, apiKey: string | null) =>
    call<number>("ai_provider_save", { config, apiKey }),
  aiProviderDelete: (provider: string) =>
    call<void>("ai_provider_delete", { provider }),

  /** Ask a model about the merchants nothing local could file. */
  aiAutocatPreview: () => call<EgressPreview>("ai_autocat_preview"),
  aiAutocatSend: (preview: EgressPreview) =>
    call<MerchantSuggestion[]>("ai_autocat_send", { preview }),
  /** One turn of the finance chat: build what would be sent, then send it. */
  aiChatPreview: (messages: ChatMessage[], scope: ChatScope) =>
    call<EgressPreview>("ai_chat_preview", { messages, scope }),
  aiChatSend: (preview: EgressPreview) => call<string>("ai_chat_send", { preview }),
  aiDecline: (preview: EgressPreview) => call<void>("ai_decline", { preview }),
  aiEgressLog: () => call<EgressEntry[]>("ai_egress_log"),

  /** Format-detection fallback: sends a header and up to four sample rows. */
  aiMappingPreview: (
    path: string,
    headerRow: number | null,
    firstDataRow: number,
  ) => call<EgressPreview>("ai_mapping_preview", { path, headerRow, firstDataRow }),
  aiMappingSend: (
    preview: EgressPreview,
    base: BankMapping,
    width: number,
  ) => call<BankMapping>("ai_mapping_send", { preview, base, width }),
};
