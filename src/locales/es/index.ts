import type { Dict } from "../en";
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

export const es: Dict = { common, nav, settings, ledger, importer, wealth, fire, simulator, assistant, setup, portfolio, plan };
