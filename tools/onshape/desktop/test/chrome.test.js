// The Onshape pane's Chromium identity and GPU switches: a normal Chrome user agent (Electron's
// own token makes Onshape treat it as an unknown browser) and WebGPU/GPU-blocklist switches that
// Linux Chromium leaves off by default.
const { test } = require('node:test');
const assert = require('node:assert/strict');

const { chromeUserAgent, gpuSwitches, applyGpuSwitches, softwareMode, softwareRelaunchArgs } = require('../chrome');

test('chrome.js: the user agent is a plain Chrome one, with no Electron/app token', () => {
  const ua = chromeUserAgent({ chromeVersion: '130.0.6723.152', platform: 'linux' });
  assert.match(ua, /^Mozilla\/5\.0 \(X11; Linux x86_64\) AppleWebKit\/537\.36 \(KHTML, like Gecko\) Chrome\/130\.0\.6723\.152 Safari\/537\.36$/);
  assert.doesNotMatch(ua, /Electron|Onshape Importer|cadrs/i);
  assert.match(chromeUserAgent({ chromeVersion: '130.0.6723.152', platform: 'win32' }), /Windows NT 10\.0; Win64; x64/);
  assert.match(chromeUserAgent({ chromeVersion: '130.0.6723.152', platform: 'darwin' }), /Macintosh; Intel Mac OS X/);
  // A missing or malformed version falls back to a valid Chrome one.
  assert.match(chromeUserAgent({ chromeVersion: 'nope' }), /Chrome\/130\.0\.0\.0 Safari/);
});

test('chrome.js: WebGPU and the GPU blocklist are asked for on Linux', () => {
  const names = gpuSwitches({}, 'linux');
  assert.deepEqual(names, [
    { name: 'ignore-gpu-blocklist' },
    { name: 'enable-unsafe-webgpu' },
    { name: 'enable-features', value: 'Vulkan' },
  ]);
  // Other platforms do not get Vulkan forced; extra features are appended.
  assert.deepEqual(gpuSwitches({}, 'win32'), [
    { name: 'ignore-gpu-blocklist' },
    { name: 'enable-unsafe-webgpu' },
  ]);
  assert.deepEqual(gpuSwitches({ CADRS_ONSHAPE_EXTRA_FEATURES: 'A, B' }, 'linux')[2].value, 'Vulkan,A,B');
});

test('chrome.js: troubleshooting switches can turn the GPU workarounds off', () => {
  assert.deepEqual(gpuSwitches({ CADRS_ONSHAPE_GPU: '0' }, 'linux'), []);
  assert.deepEqual(gpuSwitches({ CADRS_ONSHAPE_DISABLE_GPU: '1' }, 'linux'), [{ name: 'disable-gpu' }]);
});

test('chrome.js: applyGpuSwitches passes names and values to the command line', () => {
  const calls = [];
  applyGpuSwitches({ appendSwitch: (name, value) => calls.push([name, value]) }, {}, 'linux');
  assert.deepEqual(calls, [
    ['ignore-gpu-blocklist', undefined],
    ['enable-unsafe-webgpu', undefined],
    ['enable-features', 'Vulkan'],
  ]);
});

test('chrome.js: software mode is explicit, and the relaunch does not duplicate the flag', () => {
  assert.equal(softwareMode(['electron', '.'], {}), false);
  assert.equal(softwareMode(['electron', '.', '--disable-gpu'], {}), true);
  assert.equal(softwareMode(['electron', '.'], { CADRS_ONSHAPE_DISABLE_GPU: '1' }), true);
  assert.deepEqual(softwareRelaunchArgs(['app', '.', '--disable-gpu', '--x']), ['.', '--x', '--disable-gpu']);
  assert.deepEqual(softwareRelaunchArgs(['app', '.']), ['.', '--disable-gpu']);
});
