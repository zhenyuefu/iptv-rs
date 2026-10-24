#!/usr/bin/env node
'use strict';
// Stable CE launcher: update Context Engine once; repository copies need no re-vendoring.
const {spawnSync} = require('node:child_process');
const fs = require('node:fs'),path=require('node:path');
const command = process.env.CONTEXT_ENGINE_COMMAND || 'ctx';
const remote = '/home/agent/.config/context-engine/ce-client.cjs';
const useRemote = process.platform==='linux' && fs.existsSync(remote);
let root=process.cwd();
while(!fs.existsSync(path.join(root,'.agents/notes/config.json'))&&path.dirname(root)!==root)root=path.dirname(root);
const file=path.join(root,'.agents/notes/config.json');
if(fs.existsSync(file)&&JSON.parse(fs.readFileSync(file)).version>=2){
 const check=spawnSync(useRemote?process.execPath:command,useRemote?[remote,'legacy-notes','capabilities']:['notes','capabilities'],{encoding:'utf8'});
 let capabilities;try{const p=JSON.parse(check.stdout);capabilities=p.capabilities||p.result?.capabilities;}catch{}
 if(check.status!==0||!capabilities?.includes('agent-notes/2')){console.error('Agent Notes format 2 requires a CE runtime with agent-notes/2 capability.');process.exit(2);}
}
const result = spawnSync(useRemote?process.execPath:command,useRemote?[remote,'legacy-notes',...process.argv.slice(2)]:['notes',...process.argv.slice(2)],{stdio:'inherit'});
if (result.error) console.error('Context Engine is required. Install ctx on PATH or set CONTEXT_ENGINE_COMMAND: '+result.error.message);
process.exitCode = result.status ?? 1;
