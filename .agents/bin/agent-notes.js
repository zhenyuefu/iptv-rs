#!/usr/bin/env node
'use strict';
// Stable CE launcher: update Context Engine once; repository copies need no re-vendoring.
const {spawnSync} = require('node:child_process');
const command = process.env.CONTEXT_ENGINE_COMMAND || 'ctx';
const result = spawnSync(command,['notes',...process.argv.slice(2)],{stdio:'inherit'});
if (result.error) console.error('Context Engine is required. Install ctx on PATH or set CONTEXT_ENGINE_COMMAND: '+result.error.message);
process.exitCode = result.status ?? 1;
