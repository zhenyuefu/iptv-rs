#!/usr/bin/env node
'use strict';
// Stable CE launcher: update Context Engine once; repository copies need no re-vendoring.
const {spawnSync} = require('node:child_process');
const fs = require('node:fs');
const command = process.env.CONTEXT_ENGINE_COMMAND || 'ctx';
const remote = '/home/agent/.config/context-engine/ce-client.cjs';
const useRemote = process.platform==='linux' && fs.existsSync(remote);
const result = spawnSync(useRemote?process.execPath:command,useRemote?[remote,'legacy-notes',...process.argv.slice(2)]:['notes',...process.argv.slice(2)],{stdio:'inherit'});
if (result.error) console.error('Context Engine is required. Install ctx on PATH or set CONTEXT_ENGINE_COMMAND: '+result.error.message);
process.exitCode = result.status ?? 1;
