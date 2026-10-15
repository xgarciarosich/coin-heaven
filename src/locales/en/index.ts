import { assistant } from "./assistant";
import { common } from "./common";
import { nav } from "./nav";
import { plan } from "./plan";
import { fire } from "./fire";
import { importer } from "./importer";
import { ledger } from "./ledger";
import { portfolio } from "./portfolio";
import { settings } from "./settings";
import { setup } from "./setup";
import { simulator } from "./simulator";
import { wealth } from "./wealth";

export const en = { common, nav, settings, ledger, importer, wealth, fire, simulator, assistant, setup, portfolio, plan };

/** The shape every language has to fill in completely. */
export type Dict = typeof en;
