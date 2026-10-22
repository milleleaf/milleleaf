import init, { FlowSession } from './pkg/ox_web.js';
import wasmBytes from './pkg/ox_web_bytes.js';

let flow;
// The engine arrives as a script module, not fetch(), so the page can forbid every network request.
const ready = init({ module_or_path: wasmBytes }).then(() => { flow = new FlowSession(); });
let queue = Promise.resolve();
self.onmessage = ({ data: { id, method, args = [] } }) => {
  queue = queue.then(async () => {
    try {
      await ready;
      let result;
      switch (method) {
        case 'ready': result = { ready: true }; break;
        case 'sample': result = flow.sample(...args); break;
        case 'open': result = flow.open(new Uint8Array(args[0]), ...args.slice(1)); break;
        case 'inspect': result = flow.inspect(); break;
        case 'create': result = flow.create(...args); break;
        case 'import_file': result = flow.import_file(new Uint8Array(args[0]), ...args.slice(1)); break;
        case 'export_file': result = flow.export_file(...args); break;
        case 'project': result = flow.project(); break;
        case 'sign': result = flow.sign(); break;
        default: throw new Error('Unknown file operation.');
      }
      self.postMessage({ id, result: typeof result === 'string' ? JSON.parse(result) : result });
    } catch (error) { self.postMessage({ id, error: error instanceof Error ? error.message : String(error) }); }
  });
};
