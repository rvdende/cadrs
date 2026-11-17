# Windows installer and code signing

`cadrs.nsi` is the installer (NSIS; `tools/release/package.sh windows` builds it with
`makensis`, on Linux, beside the zip). It installs for the current user, with no administrator
prompt, into `%LOCALAPPDATA%\Programs\cadrs`, where the in-app updater can replace it.

## Signing (SignPath)

Windows only runs a downloaded program without asking ("Windows protected your PC") when it is
signed by a certificate that SmartScreen trusts. The releases are signed through
[SignPath](https://signpath.io), which signs open-source projects for free under the
[SignPath Foundation](https://signpath.org) certificate. The key never leaves SignPath's HSM:
the release workflow uploads the unsigned files as artifacts, SignPath fetches them from GitHub,
checks they were built by this repository's workflow, signs them and hands them back.

Two files are signed, one after the other (`.github/workflows/release.yml`, the Windows job):

1. `cadrs.exe`, before the zip and the installer take it (artifact configuration `exe`,
   [`signpath/exe.xml`](signpath/exe.xml));
2. the installer, `cadrs-v<version>-x86_64-windows-setup.exe` (artifact configuration
   `installer`, [`signpath/installer.xml`](signpath/installer.xml)).

The uninstaller isn't signed: the installer writes it on the user's machine, so it carries no
download mark and SmartScreen never checks it.

### Setting it up (once)

1. Apply for the SignPath Foundation's free open-source signing:
   https://signpath.org/apply. Its conditions include the code signing policy in the
   [README](../../README.md#code-signing-policy), which must stay true.
2. Once accepted, in the SignPath project (slug `cadrs`):
   - add a GitHub trusted build system linked to this repository;
   - add the artifact configurations `exe` and `installer` from the two XML files here;
   - the signing policy `release-signing` (the Foundation's certificate), with the approvers the
     README names.
3. In the GitHub repository (Settings › Secrets and variables › Actions):
   - secret `SIGNPATH_API_TOKEN`: a SignPath CI user's API token (a submitter on the project);
   - variable `SIGNPATH_ORGANIZATION_ID`: the SignPath organization's id;
   - variables `SIGNPATH_PROJECT_SLUG` and `SIGNPATH_POLICY_SLUG` only if the slugs aren't
     `cadrs` and `release-signing`.

Until the secret and the variable are set, releases still build, with a warning, and the
Windows files are unsigned. Try it with Actions › Release › Run workflow and "Only build": the
run's `release-windows` artifact then holds the signed zip and installer.

With the release signing policy each release waits for an approver to approve its two signing
requests in SignPath (the job waits up to an hour for each).

### Checking a signature

On Windows: the file's Properties › Digital Signatures, or
`Get-AuthenticodeSignature .\cadrs-v<version>-x86_64-windows-setup.exe`. On Linux:
`osslsigncode verify -in <file>`.

A new certificate can still get the SmartScreen prompt for a while: SmartScreen also weighs how
often a signed file has been downloaded. SignPath Foundation's certificate is shared by many
projects and has that history already.
