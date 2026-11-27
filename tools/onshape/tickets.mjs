#!/usr/bin/env node
// Ticket drafts for translation defects, one GitHub issue draft per defect group:
//
//   node tools/onshape/tickets.mjs [import-report.txt] [--raw <scraped raw dir>] [--out <dir>] [--json]
//
// Writes <out>/<dedup key>.md per group plus <out>/INDEX.md. Offline; no network is used.
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
const { main } = require(join(here, 'desktop', 'tickets.js'));
process.exitCode = main(process.argv.slice(2));
