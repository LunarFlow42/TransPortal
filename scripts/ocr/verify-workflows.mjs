import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const output = resolve(process.argv[2]);
const phase = process.argv[3] ?? 'suite';
const clients = [];
const requests = [];
const passed = [];
const exceptions = [];
let api;
let responseDelay = 0;
let nextRequest = 0;
const pause = (ms) => new Promise((accept) => setTimeout(accept, ms));
const json = async (path) => JSON.parse((await readFile(path, 'utf8')).replace(/^\uFEFF/, ''));

async function waitFor(read, description, timeout = 30_000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const value = await read();
    if (value) return value;
    await pause(100);
  }
  throw new Error(`Timed out: ${description}`);
}

async function fixture(operation, args = {}) {
  if (operation === 'select') {
    try {
      await waitFor(async () => {
        await fixture('activate');
        await pause(100);
        const value = await fixture('get'); return value.foreground === value.window;
      }, 'fixture foreground focus', 5000);
    } catch (error) {
      await writeFile(join(output, 'focus-failure.json'), JSON.stringify(await fixture('get'), null, 2));
      throw error;
    }
  }
  const id = randomUUID();
  await writeFile(join(output, 'command.json'), JSON.stringify({ id, operation, ...args }));
  return waitFor(async () => {
    let reply;
    try { reply = await json(join(output, 'reply.json')); } catch { return null; }
    if (reply.id !== id) return null;
    if (reply.error) throw new Error(reply.error);
    return reply;
  }, `fixture ${operation}`, 10_000);
}

async function targets() {
  try { return await (await fetch('http://127.0.0.1:19421/json/list', { signal: AbortSignal.timeout(1500) })).json(); }
  catch { return []; }
}

async function connect(route) {
  const target = await waitFor(async () => (await targets()).find((page) => page.url.includes(`#/${route}`)), `${route} webview`);
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((accept, reject) => { ws.addEventListener('open', accept, { once: true }); ws.addEventListener('error', reject, { once: true }); });
  const pending = new Map();
  ws.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (message.method === 'Runtime.exceptionThrown') exceptions.push(message.params.exceptionDetails.text);
    const item = pending.get(message.id);
    if (!item) return;
    pending.delete(message.id); clearTimeout(item.timer);
    if (message.error) item.reject(new Error(JSON.stringify(message.error))); else item.accept(message.result);
  });
  const send = (method, params = {}) => new Promise((accept, reject) => {
    const id = ++nextRequest;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 40_000);
    pending.set(id, { accept, reject, timer }); ws.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const result = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  await send('Runtime.enable');
  const client = { route, send, evaluate, close: () => ws.close() };
  clients.push(client);
  try {
    await waitFor(() => evaluate('Boolean(globalThis.__TAURI_INTERNALS__?.invoke && document.readyState !== "loading")').catch(() => false), `${route} IPC initialization`);
  } catch (error) {
    const diagnostics = await evaluate('({url: location.href, ready: document.readyState, tauri: Object.getOwnPropertyNames(globalThis).filter(key => key.includes("TAURI")), html: document.documentElement.outerHTML.slice(0, 6000)})').catch(String);
    await writeFile(join(output, `${route}-initialization.json`), JSON.stringify({ target, diagnostics }, null, 2));
    throw error;
  }
  return client;
}

const invoke = (client, command, args = {}) => client.evaluate(`globalThis.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`);
const store = (client, body) => client.evaluate(`(async () => { const { useAppStore } = await import('/src/store/useAppStore.ts'); ${body} })()`);
const windowCall = (client, body) => client.evaluate(`(async () => { const api = await import('/node_modules/@tauri-apps/api/window.js'); ${body} })()`);
const click = (client, label) => client.evaluate(`(() => { const button = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)}); if (!button) throw new Error('Button missing: ' + ${JSON.stringify(label)}); button.click(); })()`);
const normalized = (text) => text.replace(/\r\n?/g, '\n').trim();
const done = (description) => { passed.push(description); console.log(`PASS ${description}`); };

try {
  const shell = await connect('shell');
  const settings = await connect('settings');
  await waitFor(async () => (await invoke(settings, 'shortcut_status')).ready, 'restored startup settings');
  await waitFor(async () => (await fixture('get')).region, 'editable source window');

  if (phase === 'restart') {
    const status = await invoke(settings, 'shortcut_status');
    assert.equal(status.shortcuts.find((item) => item.id === 'double_copy').active, null);
    assert.equal(await store(settings, 'return useAppStore.getState().doubleCopyEnabled;'), false);
    assert.equal((await invoke(shell, 'auto_ocr_latest')).active, false);
    done('restart restores disabled Ctrl+C+C and does not resume screen monitoring');
  } else {
    api = createServer((request, response) => {
      let bytes = '';
      request.on('data', (chunk) => { bytes += chunk; });
      request.on('end', () => {
        let body;
        try { body = JSON.parse(bytes); } catch { response.writeHead(400).end(); return; }
        requests.push(body);
        const input = body.messages?.findLast((message) => message.role === 'user')?.content ?? '';
        const translated = input.includes('Please translate this selection') ? '你好，世界。\n请翻译这段选中文字。'
          : input.includes('Second region') ? '第二段区域文字。'
          : input.includes('Corrected') ? '校对后的文字。'
          : input.includes('First region') ? '第一段区域文字。' : '本地图片翻译成功。';
        const respond = () => {
          if (response.destroyed) return;
          if (body.stream) {
            response.writeHead(200, { 'Content-Type': 'text/event-stream' });
            response.write(`data: ${JSON.stringify({ choices: [{ delta: { content: translated } }] })}\n\n`);
            response.end('data: [DONE]\n\n');
          } else {
            response.writeHead(200, { 'Content-Type': 'application/json' });
            response.end(JSON.stringify({ choices: [{ message: { role: 'assistant', content: translated } }] }));
          }
        };
        if (responseDelay) { const timer = setTimeout(respond, responseDelay); response.on('close', () => clearTimeout(timer)); }
        else respond();
      });
    });
    await new Promise((accept) => api.listen(0, '127.0.0.1', accept));
    const base = `http://127.0.0.1:${api.address().port}/v1`;
    await store(settings, `useAppStore.getState().setProvider('openai'); useAppStore.getState().setOpenaiConfig({ api_key: 'fixture-only', base_url: ${JSON.stringify(base)} }); await useAppStore.getState().setDoubleCopyEnabled(true); return true;`);
    await waitFor(() => store(shell, `return useAppStore.getState().openaiConfig.base_url === ${JSON.stringify(base)} && useAppStore.getState().doubleCopyEnabled;`), 'provider and shortcut synchronization');
    await store(shell, 'useAppStore.setState({ pinned: true }); return true;');

    for (const engine of ['windows', 'onnx']) {
      await store(settings, `if (!await useAppStore.getState().setOcrEngine(${JSON.stringify(engine)})) throw new Error(useAppStore.getState().settingsError); return true;`);
      const id = await invoke(shell, 'ocr_image', { path: join(root, 'src-tauri/tests/fixtures/ocr/english-light.png'), mode: 'translate' });
      const result = await waitFor(async () => {
        const value = await invoke(shell, 'ocr_latest');
        if (value?.id !== id) return null;
        if (value.stage === 'error') throw new Error(value.error);
        return value.stage === 'ready' ? value : null;
      }, `${engine} image OCR`);
      assert.ok(result.picture.blocks.length >= 2);
      for (const block of result.picture.blocks) {
        assert.ok(block.x >= 0 && block.y >= 0 && block.x + block.width <= result.picture.width + 0.01 && block.y + block.height <= result.picture.height + 0.01);
      }
      await waitFor(() => store(shell, 'return !useAppStore.getState().ocrPictureBusy && useAppStore.getState().ocrPictureTranslation?.blocks.length > 0;'), `${engine} image translation`);
      assert.equal(await store(shell, 'return useAppStore.getState().ocrPictureTranslation.blocks.some(block => block.error);'), false);
      await waitFor(() => shell.evaluate('document.querySelector("canvas")?.width > 0'), 'translated image canvas');
      const changed = await shell.evaluate(`(async () => {
        const { useAppStore } = await import('/src/store/useAppStore.ts');
        const canvas = document.querySelector('canvas');
        const image = new Image(); image.src = useAppStore.getState().ocr.picture.data_url; await image.decode();
        const source = document.createElement('canvas'); source.width = canvas.width; source.height = canvas.height;
        source.getContext('2d').drawImage(image, 0, 0);
        const before = source.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
        const after = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
        let changes = 0; for (let i = 0; i < before.length; i++) if (before[i] !== after[i]) changes++;
        return changes;
      })()`);
      assert.ok(changed > 500, 'translated canvas must change pixels, not just expose text');
      const png = await shell.evaluate('document.querySelector("canvas").toDataURL("image/png")');
      const exported = join(output, `translated-${engine}.png`);
      await invoke(shell, 'export_ocr_picture', { dataUrl: png, path: exported });
      assert.ok((await readFile(exported)).length > 500);
      done(`${engine} OCR preserves geometry, draws translated pixels and exports PNG`);
    }

    const beforeCorrected = requests.length;
    await store(shell, 'await useAppStore.getState().translateOcrPicture(useAppStore.getState().ocr.picture.blocks.map(() => "Corrected OCR block.")); return true;');
    assert.ok(requests.slice(beforeCorrected).every((request) => request.messages.some((message) => message.content.includes('Corrected OCR block.'))));
    done('corrected OCR blocks use the Rust translation provider');
    for (const theme of ['light', 'dark']) {
      await store(settings, `useAppStore.getState().setTheme(${JSON.stringify(theme)}); return true;`);
      await waitFor(() => shell.evaluate(`document.documentElement.dataset.theme === ${JSON.stringify(theme)}`), `${theme} theme`);
      await pause(200);
      const screenshot = await shell.send('Page.captureScreenshot', { format: 'png' });
      await writeFile(join(output, `picture-${theme}.png`), Buffer.from(screenshot.data, 'base64'));
    }
    await store(shell, 'useAppStore.getState().cancelOcr(); useAppStore.getState().clearLookup(); return true;');

    const original = 'Hello world.\r\nPlease translate this selection.';
    assert.equal((await invoke(shell, 'shortcut_status')).shortcuts.find(item => item.id === 'double_copy').active, 'Ctrl+C+C');
    await fixture('text', { text: original });
    await fixture('readonly', { enabled: true });
    await fixture('select', { start: 0, length: original.length });
    await shell.evaluate(`globalThis.__workflowCaptures = []; globalThis.__TAURI_INTERNALS__.invoke('plugin:event|listen', { event: 'transportal://selection', target: { kind: 'Any' }, handler: globalThis.__TAURI_INTERNALS__.transformCallback(event => globalThis.__workflowCaptures.push(event.payload)) })`);
    await fixture('copy', { repeats: 1, hold: false });
    await pause(650);
    const copiedOnce = await fixture('clipboard');
    await writeFile(join(output, 'copy-input.json'), JSON.stringify(copiedOnce, null, 2));
    assert.equal(copiedOnce.foreground, copiedOnce.window, 'test source must have foreground focus during copy');
    assert.equal(normalized(copiedOnce.clipboard), normalized(original), 'ordinary copy must reach the fixture input');
    assert.equal(normalized(copiedOnce.text), normalized(original), 'copy must leave the test source unchanged');
    assert.equal(await shell.evaluate('globalThis.__workflowCaptures.length'), 0);
    await fixture('copy', { repeats: 3, hold: true });
    await pause(650);
    assert.equal(await shell.evaluate('globalThis.__workflowCaptures.length'), 0);
    await fixture('copy', { repeats: 2, hold: false });
    const capture = await waitFor(() => store(shell, 'return useAppStore.getState().capture?.replace_token ? useAppStore.getState().capture : null;'), 'native double-copy capture');
    assert.equal(normalized(capture.text), normalized(original));
    await waitFor(() => store(shell, 'return useAppStore.getState().lookupStack[0]?.status === "ready";'), 'captured selection translation');
    assert.equal(await shell.evaluate('globalThis.__workflowCaptures.length'), 1);
    done('native Ctrl+C+C triggers once; a single copy and held-key repeats do not trigger');

    await fixture('readonly', { enabled: false });
    await fixture('clipboard', { text: 'Workflow clipboard sentinel' });
    await click(shell, '替换选中文字');
    await waitFor(async () => normalized((await fixture('get')).text) === '你好，世界。\n请翻译这段选中文字。', 'translated text replacing native edit selection');
    await pause(350);
    const clipboard = await fixture('clipboard');
    assert.equal(clipboard.clipboard, 'Workflow clipboard sentinel');
    assert.equal(clipboard.marker, true, 'custom clipboard formats must survive replacement');
    await fixture('undo');
    assert.equal(normalized((await fixture('get')).text), normalized(original));
    await assert.rejects(invoke(shell, 'replace_captured_selection', { token: capture.replace_token, text: 'stale' }), /过期/);
    done('selection replacement changes the original input, preserves clipboard formats, supports Undo and consumes its token');

    await fixture('select', { start: 0, length: original.length });
    await fixture('copy', { repeats: 2, hold: false });
    const second = await waitFor(() => store(shell, `return useAppStore.getState().capture?.replace_token > ${capture.replace_token} ? useAppStore.getState().capture : null;`), 'fresh replacement target');
    await fixture('select', { start: 0, length: 5 });
    await assert.rejects(invoke(shell, 'replace_captured_selection', { token: second.replace_token, text: 'must not paste' }), /文字已变化/);
    assert.equal(normalized((await fixture('get')).text), normalized(original));
    await fixture('other');
    await waitFor(async () => { const state = await fixture('get'); return state.foreground !== state.window; }, 'other foreground window');
    await assert.rejects(invoke(shell, 'replace_captured_selection', { token: second.replace_token, text: 'must not paste' }), /焦点已转/);
    done('changed selections and other foreground windows reject replacement');

    await store(settings, 'await useAppStore.getState().setDoubleCopyEnabled(false); return true;');
    await fixture('select', { start: 0, length: original.length });
    const capturesBeforeDisabled = await shell.evaluate('globalThis.__workflowCaptures.length');
    await fixture('copy', { repeats: 2, hold: false });
    await pause(650);
    assert.equal(await shell.evaluate('globalThis.__workflowCaptures.length'), capturesBeforeDisabled);
    assert.equal(normalized((await fixture('clipboard')).clipboard), normalized(original));
    done('disabling Ctrl+C+C leaves ordinary copy working');

    await fixture('text', { text: 'First region text.\r\nAutomatic translation test.' });
    const source = await fixture('get');
    await store(shell, 'useAppStore.getState().clearLookup(); return true;');
    await windowCall(shell, 'await api.getCurrentWindow().show(); await api.getCurrentWindow().setFocus();');
    const monitorId = await invoke(shell, 'begin_auto_ocr', { options: { provider: 'openai', openai_config: { api_key: 'fixture-only', base_url: base, model: 'fixture' }, target_lang: 'zh-CN', interval_ms: 500 } });
    const selector = await connect('ocr-region');
    const selection = await invoke(selector, 'region_selection');
    assert.equal(selection.id, monitorId);
    const viewport = await selector.evaluate('({width: innerWidth, height: innerHeight})');
    const left = (source.region.x - selection.bounds.x) / selection.bounds.width * viewport.width;
    const top = (source.region.y - selection.bounds.y) / selection.bounds.height * viewport.height;
    const right = left + source.region.width / selection.bounds.width * viewport.width;
    const bottom = top + source.region.height / selection.bounds.height * viewport.height;
    await selector.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: left, y: top, button: 'left', buttons: 1, clickCount: 1 });
    await selector.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: right, y: bottom, button: 'left', buttons: 1 });
    await selector.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: right, y: bottom, button: 'left', buttons: 0, clickCount: 1 });
    const first = await waitFor(async () => { const state = await invoke(shell, 'auto_ocr_latest'); if (state.stage === 'error') throw new Error(state.error); return state.stage === 'watching' && state.frame?.translation ? state : null; }, 'selected region initial translation');
    assert.ok(first.frame.picture.blocks.some((block) => block.text.includes('First region')));
    assert.ok(first.frame.translation.blocks.some((block) => block.text.includes('第一段')));
    assert.ok(Math.abs(first.region.width - source.region.width) <= 1);
    // Put the output directly over the source: capture exclusion must still see the source.
    await windowCall(shell, `await api.getCurrentWindow().setPosition(new api.PhysicalPosition(${source.region.x}, ${source.region.y}));`);
    await pause(1200);
    const unchanged = await invoke(shell, 'auto_ocr_latest');
    assert.ok(unchanged.checks > first.checks);
    assert.equal(unchanged.frame.number, first.frame.number, 'unchanged pixels should not repeat OCR/translation');
    done('drag selection uses physical coordinates; automatic capture excludes its own output and skips unchanged frames');

    await fixture('text', { text: 'Second region text.\r\nAutomatic translation test.' });
    const changed = await waitFor(async () => { const state = await invoke(shell, 'auto_ocr_latest'); return state.frame?.number > first.frame.number && state.stage === 'watching' ? state : null; }, 'changed region translation');
    assert.ok(changed.frame.picture.blocks.some((block) => block.text.includes('Second region')));
    assert.ok(changed.frame.translation.blocks.some((block) => block.text.includes('第二段')));
    await invoke(shell, 'pause_auto_ocr', { paused: true });
    const paused = await invoke(shell, 'auto_ocr_latest');
    await fixture('text', { text: 'A paused region must not be captured.' });
    await pause(1100);
    assert.equal((await invoke(shell, 'auto_ocr_latest')).checks, paused.checks);
    done('content changes update the translated picture; pause stops capture');

    responseDelay = 5000;
    await invoke(shell, 'pause_auto_ocr', { paused: false });
    await waitFor(async () => (await invoke(shell, 'auto_ocr_latest')).stage === 'translating', 'in-flight translation before stop');
    await fixture('escape');
    await waitFor(async () => !(await invoke(shell, 'auto_ocr_latest')).active, 'global Escape stops monitoring');
    await pause(150);
    const stopped = await invoke(shell, 'auto_ocr_latest');
    await pause(5200);
    assert.equal((await invoke(shell, 'auto_ocr_latest')).revision, stopped.revision);
    assert.equal((await invoke(shell, 'auto_ocr_latest')).frame.translation, null);
    responseDelay = 0;
    done('Esc cancels an in-flight provider request and late results cannot revive monitoring');

    // Reuse the real selector for a blank source; no old translation may remain.
    await fixture('text', { text: '' });
    const blankId = await invoke(shell, 'begin_auto_ocr', { options: { provider: 'openai', openai_config: { api_key: 'fixture-only', base_url: base, model: 'fixture' }, target_lang: 'zh-CN', interval_ms: 500 } });
    const blankSelection = await waitFor(async () => { const value = await invoke(selector, 'region_selection'); return value?.purpose === 'monitor' && value.id !== selection.id ? value : null; }, 'second monitor selection');
    await invoke(selector, 'confirm_region', { id: blankSelection.id, region: source.region });
    const blank = await waitFor(async () => { const state = await invoke(shell, 'auto_ocr_latest'); return state.id === blankId && state.frame?.translation && state.stage === 'watching' ? state : null; }, 'blank region result');
    assert.equal(blank.frame.picture.blocks.length, 0);
    assert.equal(blank.frame.translation.blocks.length, 0);
    await invoke(shell, 'stop_auto_ocr', { discard: true });
    done('blank regions clear old text and stop releases the image');
    await pause(500);
  }
  assert.deepEqual(exceptions, []);
  await writeFile(join(output, `${phase}.json`), JSON.stringify({ passed, request_count: requests.length, exceptions }, null, 2));
} catch (error) {
  const shell = clients.find((client) => client.route === 'shell');
  const diagnostics = shell ? await store(shell, 'const auto = useAppStore.getState().autoOcr; return { capture: useAppStore.getState().capture, events: globalThis.__workflowCaptures, lookup: useAppStore.getState().lookupStack.map(node => ({text: node.text, status: node.status})), error: useAppStore.getState().errorMessage, monitor: auto && {id: auto.id, stage: auto.stage, error: auto.error, selecting: auto.selecting, region: auto.region} };').catch(String) : null;
  if (diagnostics && typeof diagnostics === 'object') {
    diagnostics.windows = await windowCall(shell, 'return (await api.getAllWindows()).map(window => window.label);').catch(String);
    diagnostics.debugPages = (await targets()).map(({ title, url }) => ({ title, url }));
  }
  await writeFile(join(output, `${phase}-failure.json`), JSON.stringify({ passed, error: error.stack, exceptions, diagnostics }, null, 2));
  console.error(error);
  process.exitCode = 1;
} finally {
  for (const client of clients) client.close();
  if (api) { api.closeAllConnections(); api.close(); }
}
