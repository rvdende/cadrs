// The Chromium identity and GPU switches the Onshape pane needs.
//
// Two things made the embedded Onshape pane unusable on a stock Linux Electron:
//  - Electron puts itself in the user agent (`… Electron/33 … cadrs Onshape Importer/0.1.0 …`);
//    Onshape looks at the browser it is given and can decide it cannot draw, sending the user to
//    its graphics help pages. Present a normal Chrome user agent instead.
//  - Chromium keeps WebGPU off (and ignores blocklisted GPUs) unless asked; Onshape's graphics
//    check wants WebGPU/WebGL2. Turn those on before the app is ready.
//
// The switches are conservative and can be turned off for troubleshooting (see `gpuSwitches`).

/** A plain Chrome user agent for the current platform (no Electron/app token). */
function chromeUserAgent(options = {}) {
  const platform = options.platform || process.platform;
  const chrome = options.chromeVersion || process.versions.chrome || '130.0.0.0';
  const version = /^\d+\.\d+\.\d+\.\d+$/.test(chrome) ? chrome : '130.0.0.0';
  const tokens = {
    win32: 'Windows NT 10.0; Win64; x64',
    darwin: 'Macintosh; Intel Mac OS X 10_15_7',
  };
  const token = tokens[platform] || 'X11; Linux x86_64';
  return `Mozilla/5.0 (${token}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${version} Safari/537.36`;
}

/** The Chromium switches to start with, as `{ name, value? }` entries.
 *
 *    CADRS_ONSHAPE_DISABLE_GPU=1    software rendering (`--disable-gpu`)
 *    CADRS_ONSHAPE_GPU=0            no GPU/WebGPU switches at all
 *    CADRS_ONSHAPE_EXTRA_FEATURES=a,b   more `--enable-features` entries
 */
function gpuSwitches(env = process.env, platform = process.platform) {
  if (env.CADRS_ONSHAPE_GPU === '0') return [];
  if (env.CADRS_ONSHAPE_DISABLE_GPU === '1') return [{ name: 'disable-gpu' }];
  const switches = [
    { name: 'ignore-gpu-blocklist' },
    { name: 'enable-unsafe-webgpu' },
  ];
  // WebGPU on Linux is Vulkan-backed; ask Chromium to use it (other platforms expose WebGPU
  // without this, and forcing Vulkan there could pick a broken driver).
  const features = [];
  if (platform === 'linux') features.push('Vulkan');
  if (env.CADRS_ONSHAPE_EXTRA_FEATURES) {
    features.push(...env.CADRS_ONSHAPE_EXTRA_FEATURES.split(',').map((f) => f.trim()).filter(Boolean));
  }
  if (features.length) switches.push({ name: 'enable-features', value: features.join(',') });
  return switches;
}

/** Applies `switches` to an Electron `app.commandLine`-like object. */
function applyGpuSwitches(commandLine, env = process.env, platform = process.platform) {
  for (const { name, value } of gpuSwitches(env, platform)) {
    if (value === undefined) commandLine.appendSwitch(name);
    else commandLine.appendSwitch(name, value);
  }
}

module.exports = { chromeUserAgent, gpuSwitches, applyGpuSwitches };
