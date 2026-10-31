#!/usr/bin/env python3
"""Copy/run the guest benchmark through the engine; requires the app already running."""
import argparse,json,pathlib,subprocess
p=argparse.ArgumentParser(description=__doc__);p.add_argument('app');p.add_argument('--cli',default='defuss-container');p.add_argument('--engine',required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args()
info=json.loads(subprocess.check_output([a.cli,'show',a.app],text=True));machine=info['application']['machine']
source=pathlib.Path(__file__).resolve().parents[1]/'guest/benchmark.mjs';remote='/tmp/defuss-benchmark.mjs'
subprocess.run([a.engine,'machine','cp',str(source),f'{machine}:{remote}'],check=True)
output=subprocess.check_output([a.engine,'machine','exec','--name',machine,'--','/usr/local/bin/bun',remote],text=True)
data=json.loads(output);a.output.write_text(json.dumps(data,indent=2)+'\n');print(a.output)
