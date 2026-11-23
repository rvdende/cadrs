#!/usr/bin/env node
// JS replacement for tools/onshape/roots.py (same output format, no python3 needed):
//
//   node tools/onshape/roots.mjs [import-report.txt] [--show N]
//
// The first feature each Part Studio could not import, grouped by reason.
// Default report: $CADRS_ONSHAPE_ROOT/import-report.txt, else ~/work/cadrs_onshape/...
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
const { main } = require(join(here, 'desktop', 'reports.js'));
process.exitCode = main(['roots', ...process.argv.slice(2)]);
