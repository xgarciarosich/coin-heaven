/**
 * Mirrors `src-tauri/src/sim`. Kept hand-written and small on purpose; if this
 * surface grows, generate it from the Rust types (ts-rs) rather than letting
 * the two drift.
 *
 * Every `Minor` field is an integer number of cents — never a float.
 */
import type { Minor } from "./money";

/** One purchase the person intends to make. */
export interface PlannedProperty {
  name: string;
  /** Month of the projection, 1-based. */
  buyMonth: number;
  price: Minor;
  renovation: Minor;
  /** Gross rent a month when first let. */
  monthlyRent: Minor;
  /** Purchase tax: it depends on the region, so it lives with the property. */
  transferTaxBps: number;
  /** `null` takes the general value. */
  financingBps: number | null;
  mortgageRateBps: number | null;
  mortgageTermMonths: number | null;
  fixedCostsAnnual: Minor | null;
  /**
   * Bought before the plan starts: held from the first month at `price` (its
   * value today) and `monthlyRent` (today's rent), with the loan it still has.
   * Its mortgage terms are the rate and the months left. `null` for a planned
   * purchase.
   */
  owned?: OwnedSince | null;
}

export interface OwnedSince {
  /** "YYYY-MM" */
  boughtOn: string | null;
  paid: Minor | null;
  loanRemaining: Minor;
}

export interface SimParams {
  horizonYears: number;
  /** "YYYY-MM": the calendar month that is month 1 of the plan. */
  startMonth: string | null;

  startCash: Minor;
  monthlySavings: Minor;
  netMonthlySalary: Minor;
  /** Yearly rise in pay; what is put aside rises with it. */
  incomeGrowthBps: number;
  existingMonthlyDebt: Minor;
  cashBuffer: Minor;
  reinvestNetRent: boolean;

  financingBps: number;
  mortgageRateBps: number;
  mortgageTermMonths: number;
  otherClosingBps: number;
  dtiMaxBps: number;
  computableRentBps: number;

  occupancyBps: number;
  rentCpiBps: number;
  appreciationBps: number;
  mgmtFeeBps: number;
  maintenanceBps: number;
  fixedCostsAnnual: Minor;
  rentalTaxBps: number;

  properties: PlannedProperty[];
}

export interface YearRow {
  year: number;
  savingsContributed: Minor;
  netRentReceived: Minor;
  purchases: number;
  properties: number;
  cash: Minor;
  grossRentMonthly: Minor;
  netRentMonthly: Minor;
  portfolioValue: Minor;
  debt: Minor;
  equity: Minor;
  /** What is yours in each planned property at year end, in plan order. */
  propertyEquity: Minor[];
  netWorth: Minor;
  salaryMonthly: Minor;
  dtiBps: number;
  dtiOk: boolean;
}

/** What became of one planned purchase. */
export interface PropertyOutcome {
  index: number;
  plannedMonth: number;
  /** `null` when it could not be bought within the horizon. */
  boughtMonth: number | null;
  blockedBy: "cash" | "bank" | null;
  cashNeeded: Minor;
  cashShort: Minor;
  loan: Minor;
  monthlyPayment: Minor;
  dtiBpsPlanned: number;
  netRentFirst: Minor;
  valueEnd: Minor;
  debtEnd: Minor;
  netRentEnd: Minor;
}

export interface SimResult {
  years: YearRow[];
  monthlyNetWorth: Minor[];
  properties: PropertyOutcome[];
  finalNetWorth: Minor;
  finalProperties: number;
  finalNetRentMonthly: Minor;
  /** Put aside from pay plus rent kept, a month, at the end. */
  finalSavingCapacity: Minor;
}

export interface ParamError {
  field: string;
  message: string;
}

export interface AppStatus {
  schemaVersion: number;
  unlocked: boolean;
  dbPath: string;
  version: string;
}

export interface AppErrorPayload {
  kind:
    | "db"
    | "migration"
    | "keystore"
    | "locked"
    | "invalid"
    | "sim_params"
    | "network"
    | "other";
  message: string;
  details: ParamError[] | null;
}

// --- unlock / recovery ------------------------------------------------------

export type UnlockState =
  /** No database yet — ask how the user wants it protected. */
  | "needs_setup"
  | "opened"
  /** Protected by a passphrase, which is cached nowhere. */
  | "needs_passphrase"
  /** No passphrase, but the cached key is gone. */
  | "needs_recovery_phrase"
  /** Created before the envelope scheme; needs a one-time upgrade. */
  | "needs_legacy_upgrade";

export interface UnlockStatus {
  state: UnlockState;
  hasPassphrase: boolean;
  phrasePending: boolean;
  /** They typed it back, so we know they have it. */
  phraseConfirmed: boolean;
  /** They got past the phrase screen, possibly by deferring. */
  phraseAcknowledged: boolean;
}

export interface PassphraseStrength {
  /** zxcvbn 0–4. */
  score: number;
  /** False only when it is too short; weakness is advised against, not blocked. */
  acceptable: boolean;
  message: string;
}

// --- backup -----------------------------------------------------------------

export interface BackupStatus {
  enabled: boolean;
  directory: string | null;
  minIntervalSeconds: number;
  retainCopies: number;
  dirty: boolean;
  lastBackupAt: string | null;
  lastOutcome: string | null;
}

export type BackupOutcome =
  | { written: { path: string; bytes: number } }
  | "skippedNotDue"
  | "skippedClean"
  | "disabled"
  | "noDirectory";

// --- FIRE -------------------------------------------------------------------

export interface FireParams {
  annualSpendTarget: Minor;
  swrBps: number;
  annualNetRentalIncome: Minor;
}

export interface Balances {
  liquid: Minor;
  realEstateEquity: Minor;
  otherAssets: Minor;
  otherDebt: Minor;
}

export interface FireResult {
  liquidTarget: Minor;
  liquidAssets: Minor;
  liquidShortfall: Minor;
  liquidProgressBps: number;
  liquidCovered: boolean;
  /** Deliberately separate from the liquid figures — never add the two. */
  totalNetWorth: Minor;
  rentCoverageBps: number;
  coveredByRentAlone: boolean;
  residualAnnualSpend: Minor;
}

// --- matching ---------------------------------------------------------------

export interface TransferCandidate {
  outId: number;
  inId: number;
  residualMinor: Minor;
  dayGap: number;
}

export interface SplitCandidate {
  parentId: number;
  reimbursementId: number;
  fractionDenominator: number;
  residualMinor: Minor;
  dayGap: number;
}

// --- currency ---------------------------------------------------------------

export interface Currency {
  code: string;
  name: string;
  /** Decimal places. 2 for EUR, 0 for JPY, 3 for KWD. */
  exponent: number;
  symbol: string;
}

// --- import -----------------------------------------------------------------

export interface BankMapping {
  name: string;
  delimiter: string;
  encoding: string;
  hasHeader: boolean;
  skipRows: number;
  dateCol: number;
  dateFormat: string;
  descriptionCol: number;
  /** The single signed amount column. Outgoings negative; see invertSign. */
  amountCol: number | null;
  counterpartyCol: number | null;
  decimalSeparator: string;
  thousandsSeparator: string;
  invertSign: boolean;
  currency: string;
}

export interface StoredMapping extends BankMapping {
  id: number;
}

export interface Account {
  id: number;
  name: string;
  institution: string | null;
  kind: string;
  currency: string;
  txnCount: number;
}

export interface ParsedRow {
  line: number;
  bookedOn: string;
  amountMinor: Minor;
  description: string;
  counterparty: string | null;
}

export interface Rejection {
  line: number;
  reason: string;
  raw: string;
}

export type Source =
  | { delimited: { delimiter: string; encoding: string } }
  | { spreadsheet: { sheet: string } };

export interface Detection {
  mapping: BankMapping;
  /** 0–100. Below ~70 the UI asks the user to confirm. */
  confidence: number;
  notes: string[];
  warnings: string[];
  headerRow: number | null;
  firstDataRow: number;
}

export interface Inspection {
  detection: Detection;
  rows: ParsedRow[];
  rejected: Rejection[];
  headers: string[];
  sampleLines: string[];
  totalRows: number;
  source: Source;
  needsConfirmation: boolean;
}

export interface MappingHint {
  dateCol: number | null;
  dateFormat: string | null;
  descriptionCol: number | null;
  amountCol: number | null;
  decimalSeparator: string | null;
  thousandsSeparator: string | null;
}

export interface Preview {
  rows: ParsedRow[];
  rejected: Rejection[];
  headers: string[];
  sampleLines: string[];
  totalRows: number;
}

export interface ImportSummary {
  batchId: number;
  rowCount: number;
  inserted: number;
  duplicates: number;
  rejected: number;
}

export interface Batch {
  id: number;
  accountName: string;
  sourceFilename: string | null;
  importedAt: string;
  insertedCount: number;
  duplicateCount: number;
  rejectedCount: number;
}

/** One side of a proposed pair, described well enough to recognise. */
export interface MatchSide {
  id: number;
  bookedOn: string;
  description: string;
  counterparty: string | null;
  accountName: string;
  amount: Minor;
  currency: string;
}

export interface PendingTransfer {
  outId: number;
  inId: number;
  out: MatchSide;
  incoming: MatchSide;
  /** `out + in`. Zero for a clean pair; anything else is the fee. */
  residualMinor: Minor;
  dayGap: number;
  reason: string;
  /** Both legs name the same account — worth a second look. */
  sameAccountName: boolean;
}

export interface PendingSplit {
  parentId: number;
  reimbursementId: number;
  parent: MatchSide;
  reimbursement: MatchSide;
  fractionDenominator: number;
  residualMinor: Minor;
  dayGap: number;
  reason: string;
  /** What the bill nets to if this link is confirmed. */
  parentNetAfter: Minor;
  existingLinks: number;
}

// --- ledger (categories, events, transactions) -------------------------------

export interface Category {
  id: number;
  parentId: number | null;
  name: string;
  kind: "income" | "expense" | "neutral";
  color: string | null;
  sortOrder: number;
  txnCount: number;
}

export interface EventItem {
  id: number;
  name: string;
  defaultCategoryId: number;
  defaultCategoryName: string;
  startsOn: string | null;
  endsOn: string | null;
  budgetMinor: Minor | null;
  isClosed: boolean;
  txnCount: number;
  /** Net of reimbursements booked against the event's transactions. */
  netMinor: Minor;
}

export interface EventInput {
  name: string;
  defaultCategoryId: number;
  startsOn: string | null;
  endsOn: string | null;
  budgetMinor: number | null;
  notes: string | null;
}

export interface TxnFilter {
  accountId?: number | null;
  projectId?: number | null;
  categoryId?: number | null;
  from?: string | null;
  to?: string | null;
  search?: string | null;
  needsReview?: boolean | null;
  limit?: number | null;
  offset?: number | null;
}

export interface LedgerRow {
  id: number;
  bookedOn: string;
  description: string;
  counterparty: string | null;
  amountMinor: Minor;
  netMinor: Minor;
  currency: string;
  accountId: number;
  accountName: string;
  notes: string | null;
  /** The shop a line is from; lines with the same one belong together. */
  merchant: string | null;
  categoryId: number | null;
  categoryName: string | null;
  projectId: number | null;
  projectName: string | null;
  isNeutralTransfer: boolean;
  needsReview: boolean;
  parentTxnId: number | null;
  reimbursementCount: number;
}

export interface Ledger {
  rows: LedgerRow[];
  total: number;
}

export interface PlLine {
  categoryId: number | null;
  categoryName: string;
  kind: string;
  color: string | null;
  netMinor: Minor;
  txnCount: number;
}

export interface PlSummary {
  lines: PlLine[];
  incomeMinor: Minor;
  expenseMinor: Minor;
  netMinor: Minor;
  uncategorisedCount: number;
}

// --- net worth ---------------------------------------------------------------

/** An asset class key. User-defined, so any string the backend knows. */
export type Bucket = string;

/** What the arithmetic does with a class. Not user-defined. */
export type AssetKind = "liquid" | "property" | "other_asset" | "liability";

export interface AssetClass {
  key: string;
  name: string;
  kind: AssetKind;
  color: string | null;
  sortOrder: number;
  /** Whether what is held in it counts toward the FIRE portfolio. */
  countsFire: boolean;
  lineCount: number;
}

export interface WealthLine {
  id: number | null;
  bucket: Bucket;
  label: string;
  grossValueMinor: number;
  debtMinor: number;
  currency: string;
  /** "manual", or the source of an automatic line. */
  source?: "manual" | "account" | "loan" | "holding";
}

export interface Snapshot {
  id: number;
  takenOn: string;
  label: string | null;
  isScheduled: boolean;
  grossAssetsMinor: Minor;
  totalDebtMinor: Minor;
  netWorthMinor: Minor;
  liquidMinor: Minor;
  reEquityMinor: Minor;
  lines: WealthLine[];
}

export interface SnapshotInput {
  takenOn: string;
  label: string | null;
  lines: WealthLine[];
}

export interface SnapshotPrompt {
  due: boolean;
  suggestedDate: string;
  label: string;
}

export interface FireView {
  params: FireParams;
  balances: Balances;
  result: FireResult;
  hasSnapshot: boolean;
}

// --- assistant ---------------------------------------------------------------

export interface AiSettings {
  enabled: boolean;
  requirePreview: boolean;
  allowRawDescriptions: boolean;
  /** A self-hosted search engine (SearXNG) for models that cannot search. */
  searchUrl: string;
  /** Who answers a model's searches. */
  searchEngine: "off" | "duckduckgo" | "exa" | "searxng";
  /** Whether an Exa key is stored. The key itself is never sent back. */
  exaKeySet: boolean;
  /** On the way in only: omit to keep, "" to remove, text to replace. */
  exaKey?: string | null;
}

export interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

/** Which parts of the person's records a chat may draw on. */
export interface ChatScope {
  cashflow: boolean;
  netWorth: boolean;
  portfolio: boolean;
  fire: boolean;
}

/** A category a model proposes for one merchant group. */
export interface MerchantSuggestion {
  key: string;
  categoryId: number;
  categoryName: string;
  confidenceBps: number;
}

export interface ProviderConfig {
  provider: string;
  displayName: string | null;
  model: string;
  baseUrl: string | null;
  isEnabled: boolean;
  maxOutputTokens: number | null;
  temperature: number | null;
  systemPrompt: string | null;
}

export interface ProviderRow extends ProviderConfig {
  id: number;
  hasKey: boolean;
  isLocal: boolean;
  requiresKey: boolean;
  defaultBaseUrl: string;
}

export interface Redactions {
  ibans: number;
  cards: number;
  emails: number;
  phones: number;
  accountNumbers: number;
}

/** Exactly what would be transmitted, assembled before anything is sent. */
export interface EgressPreview {
  provider: string;
  model: string;
  purpose: string;
  payload: string;
  payloadSha256: string;
  bytes: number;
  rowCount: number;
  redactions: Redactions;
}

export interface EgressEntry {
  id: number;
  occurredAt: string;
  provider: string;
  model: string;
  purpose: string;
  payloadSha256: string;
  payloadPreview: string;
  rowCount: number | null;
  requestBytes: number | null;
  responseBytes: number | null;
  outcome: string;
}

// --- global settings ---------------------------------------------------------

export interface ModuleToggles {
  import: boolean;
  ledger: boolean;
  wealth: boolean;
  fire: boolean;
  simulator: boolean;
  assistant: boolean;
}

export interface AppSettings {
  /** Presentation only — symbol, grouping, decimals. Converts nothing. */
  primaryCurrency: string;
  locale: string;
  /** "en" | "es", or "" until someone has chosen. */
  language: string;
  /** A region id from `/regions`. */
  region: string;
  /** That region's country in English, for the assistant. */
  country: string;
  /** Look for a newer release when the app starts. Off until chosen. */
  checkUpdates: boolean;
  modules: ModuleToggles;
}

export interface CategoryUsage {
  transactions: number;
  events: number;
  rules: number;
}

// --- cash flow ---------------------------------------------------------------

export interface TxnEdit {
  bookedOn?: string | null;
  description?: string | null;
  amountMinor?: number | null;
  accountId?: number | null;
  counterparty?: string | null;
  categoryId?: number | null;
  clearCategory?: boolean;
  projectId?: number | null;
  clearProject?: boolean;
  parentTxnId?: number | null;
  clearParent?: boolean;
  isNeutralTransfer?: boolean | null;
  notes?: string | null;
}

export interface PeriodTotals {
  /** "2026-03" for a month, "2026" for a year. */
  period: string;
  incomeMinor: Minor;
  expenseMinor: Minor;
  netMinor: Minor;
}

export interface CashflowSummary {
  incomeMinor: Minor;
  expenseMinor: Minor;
  netMinor: Minor;
  /** Net ÷ income in bps; null when nothing came in. */
  savingsRateBps: number | null;
  transactionCount: number;
}

// --- categorisation rules ----------------------------------------------------

export interface Rule {
  id: number | null;
  name: string;
  priority: number;
  matchKind: "contains" | "starts_with" | "exact" | "regex";
  pattern: string;
  accountId: number | null;
  minAmountMinor: number | null;
  maxAmountMinor: number | null;
  setCategoryId: number | null;
  setProjectId: number | null;
  setNeutralTransfer: boolean;
  isEnabled: boolean;
}

// --- date formats ------------------------------------------------------------

export interface DateFormat {
  /** chrono strftime pattern — what gets stored on the mapping. */
  pattern: string;
  example: string;
  label: string;
  group: string;
  hasTime: boolean;
}

// --- the category x month matrix, budgets, auto-categorisation ---------------

export interface MatrixRow {
  categoryId: number | null;
  categoryName: string;
  kind: string;
  color: string | null;
  /** One entry per period, aligned to `Matrix.periods`. */
  cells: Minor[];
  counts: number[];
  totalMinor: Minor;
  /** The plan for the whole year, if one is set. */
  budgetMinor: Minor | null;
}

export interface Matrix {
  periods: string[];
  rows: MatrixRow[];
  incomeByPeriod: Minor[];
  expenseByPeriod: Minor[];
  netByPeriod: Minor[];
  incomeTotal: Minor;
  expenseTotal: Minor;
  netTotal: Minor;
  /** Months with any activity, so averages divide by a real number. */
  activePeriods: number;
}

/** One period of one category's plan: a year, or a month of it. */
export interface BudgetCell {
  /** "2026" for a year, "2026-03" for a month. */
  period: string;
  /** Null when there is no plan for this period. */
  amountMinor: Minor | null;
  /** Written for this period, rather than worked out from the others. */
  own: boolean;
  /** Positive magnitude of what actually happened. */
  actualMinor: Minor;
  remainingMinor: Minor;
  txnCount: number;
  elapsedBps: number;
  daysTotal: number;
  daysLeft: number;
}

/** A category's budget for a year, as the year and as its twelve months. */
export interface BudgetPlan {
  categoryId: number;
  categoryName: string;
  kind: string;
  color: string | null;
  monthsSet: number;
  monthsSumMinor: Minor;
  year: BudgetCell;
  months: BudgetCell[];
}

/** One box of a month's plan: a named amount of income or spending. */
export interface PlanItem {
  kind: "income" | "expense";
  name: string;
  /** Always positive; the kind decides which way it goes. */
  amountMinor: Minor;
  /** In a plan for two: one person's, the other's, or shared. */
  owner: "shared" | "a" | "b";
}

/** Who a month's plan is for, and how a couple splits what they share. */
export interface PlanSetup {
  mode: "individual" | "couple";
  personA: string;
  personB: string;
  /** Shared costs: half each, or in proportion to what each brings in. */
  split: "equal" | "income";
}

/** A month's plan, boxes in the order the person arranged them. */
export interface MonthPlan {
  /** "2026-10". */
  period: string;
  setup: PlanSetup;
  items: PlanItem[];
  incomeMinor: Minor;
  expenseMinor: Minor;
  netMinor: Minor;
}

/** A payment that comes round on a rhythm, found in the ledger. */
export interface Recurring {
  name: string;
  categoryName: string | null;
  every: "weekly" | "fortnightly" | "monthly" | "quarterly" | "half-yearly" | "yearly";
  typicalMinor: Minor;
  varies: boolean;
  count: number;
  firstOn: string;
  lastOn: string;
  nextOn: string;
  yearlyMinor: Minor;
  active: boolean;
}

export interface RecurringReport {
  items: Recurring[];
  yearlyMinor: Minor;
  monthlyMinor: Minor;
}

export interface ExportOutcome {
  folder: string;
  files: { name: string; rows: number }[];
}

export interface SampleStatus {
  loaded: boolean;
  canLoad: boolean;
}

export interface UpdateInfo {
  current: string;
  latest: string;
  url: string;
  newer: boolean;
}

/** Holdings with buys that have no price on their own day. */
export interface MarketAsk {
  holdingId: number;
  name: string;
  symbol: string | null;
  assetClass: string;
  buysMissing: number;
  from: string;
  source: string;
}

export type MarketProblem =
  | "not_found"
  | "other_currency"
  | "no_data"
  | "rate_limited"
  | "unreachable";

export interface MarketResult {
  holdingId: number;
  name: string;
  symbolUsed: string | null;
  currency: string | null;
  stored: number;
  leftOut: number;
  from: string | null;
  to: string | null;
  problem: MarketProblem | null;
}

/** What removing an import would delete. */
export interface ImportImpact {
  filename: string | null;
  total: number;
  /** Of those, ones the person has filed, linked or put in an event. */
  touched: number;
  linkedFromElsewhere: number;
}

export interface BudgetSeed {
  categoryId: number;
  amountMinor: Minor;
}

export interface SampleTxn {
  id: number;
  bookedOn: string;
  description: string;
  amountMinor: Minor;
}

export interface SuggestedGroup {
  key: string;
  txnIds: number[];
  txnCount: number;
  totalMinor: Minor;
  firstOn: string;
  lastOn: string;
  samples: SampleTxn[];
  suggestedCategoryId: number | null;
  suggestedCategoryName: string | null;
  source: "rule" | "history" | "none";
  ruleName: string | null;
  confidenceBps: number;
  evidenceCount: number;
}

export interface Suggestions {
  groups: SuggestedGroup[];
  uncategorisedTotal: number;
  covered: number;
}

export interface Assignment {
  txnIds: number[];
  /** null leaves the category to the event's default. */
  categoryId: number | null;
  eventId: number | null;
  createRule: boolean;
  key: string;
}

export interface ApplyOutcome {
  categorised: number;
  rulesCreated: number;
}

// --- the road to independence ------------------------------------------------

export interface PlanParams {
  annualSavings: Minor;
  /** Real return, above inflation, in basis points. 500 = 5%. */
  realReturnBps: number;
  currentAge: number | null;
  targetAge: number | null;
  horizonYears: number;
  /** Gross withdrawals up for tax on investment gains. */
  withdrawalTax: boolean;
  /** Share of each withdrawal that is gain, in bps. */
  gainShareBps: number;
  pensionAge: number | null;
  annualPension: Minor;
  /** Pessimistic/optimistic band either side of the real return, in bps. */
  bandBps: number;
  /** The tax table withdrawals are grossed up with; editable. */
  taxBrackets: TaxBracket[];
}

export interface TaxBracket {
  /** `null` on the last band: no upper limit. */
  upTo: Minor | null;
  rateBps: number;
}

/** A wished-for retirement age, held up against the plan. */
export interface Goal {
  age: number;
  yearsAvailable: number;
  onTrack: boolean;
  spareYears: number | null;
  liquidAtGoal: Minor;
  targetAtGoal: Minor;
  extraSavingMonthly: Minor | null;
  spendCutMonthly: Minor | null;
}

export interface PlanPoint {
  /** 0 is today. */
  year: number;
  age: number | null;
  liquid: Minor;
  liquidLow: Minor;
  liquidHigh: Minor;
  target: Minor;
}

export interface Sensitivity {
  swrBps: number;
  target: Minor;
  years: number | null;
}

export interface Plan {
  path: PlanPoint[];
  /** Null when the target is not reached within the horizon. */
  yearsToTarget: number | null;
  yearsLow: number | null;
  yearsHigh: number | null;
  ageAtTarget: number | null;
  /** The portfolio needed to stop today, with tax and pension applied. */
  targetNow: Minor;
  grossWithdrawal: Minor;
  withdrawalTax: Minor;
  coastNumber: Minor | null;
  coasting: boolean;
  goal: Goal | null;
  sensitivity: Sensitivity[];
  milestones: Milestone[];
  levers: Lever[];
}

export interface Milestone {
  shareBps: number;
  years: number | null;
  age: number | null;
}

export interface Lever {
  kind: "save" | "spend";
  monthly: Minor;
  monthsEarlier: number | null;
}

export interface LedgerSuggestion {
  from: string;
  to: string;
  monthsWithData: number;
  annualSpending: Minor;
  annualIncome: Minor;
  annualSaving: Minor;
  savingsRateBps: number | null;
}

export interface FirePlanView {
  params: FireParams;
  planParams: PlanParams;
  balances: Balances;
  result: FireResult;
  plan: Plan;
  hasSnapshot: boolean;
  /** Nothing saved yet: the plan is the built-in starting point. */
  planIsNew: boolean;
  /** Likewise for the withdrawal rate and spending target. */
  paramsIsNew: boolean;
}

// --- portfolio --------------------------------------------------------------

export interface Schedule {
  id: number | null;
  holdingId: number;
  amountMinor: Minor;
  everyDays: number | null;
  dayOfMonth: number | null;
  startDate: string;
  endDate: string | null;
}

export interface HoldingView {
  id: number;
  name: string;
  symbol: string | null;
  assetClass: string;
  /** Units ×1e8. */
  unitsE8: number;
  unitsComplete: boolean;
  /** The count leans on a figure Coin Heaven worked out rather than one typed. */
  unitsEstimated: boolean;
  /** € per unit ×1e6. */
  priceMicro: number | null;
  priceDate: string | null;
  priceSource: string | null;
  priceUrl: string | null;
  value: Minor | null;
  invested: Minor;
  gain: Minor | null;
  xirrBps: number | null;
  txnCount: number;
  schedules: Schedule[];
}

export interface PortfolioView {
  asOf: string;
  holdings: HoldingView[];
  value: Minor;
  invested: Minor;
  gain: Minor;
  xirrBps: number | null;
  unpriced: string[];
  /** The latest 30 June or 31 December. */
  checkpoint: string;
}

export interface HoldingTxn {
  id: number;
  onDate: string;
  kind: "buy" | "sell" | "units";
  amountMinor: Minor;
  unitsE8: number | null;
  scheduled: boolean;
  skipped: boolean;
}

export interface PriceInput {
  holdingId: number;
  onDate: string;
  priceMicro: number;
  source: "manual" | "assistant";
  sourceUrl: string | null;
}

/** One point of the portfolio's history. */
export interface HistoryPoint {
  date: string;
  value: Minor;
  invested: Minor;
}

export interface PriceProposal {
  holdingId: number;
  name: string;
  priceMicro: number | null;
  priceDate: string | null;
  /** A past price fetched to fill in history, not the current one. */
  historical: boolean;
  sourceUrl: string | null;
  /** The source is among the pages the search actually returned. */
  sourceSeen: boolean;
  /** The words the price was read from, checked against the search results. */
  evidence?: string | null;
  note: string | null;
}
