#!/usr/bin/env node
// Gate every runtime advisory and every new tooling advisory. The existing
// braces advisory has no patched release and reaches this repo only through
// Next's ESLint tooling: https://github.com/advisories/GHSA-vfj7-8cjw-p6xm
import { spawnSync } from 'node:child_process'

function audit(extraArgs = []) {
  const result = spawnSync('npm', ['audit', '--json', ...extraArgs], {
    encoding: 'utf8',
    maxBuffer: 10 * 1024 * 1024,
  })
  if (result.error || ![0, 1].includes(result.status)) {
    throw new Error(`npm audit could not complete: ${result.error?.message ?? result.stderr}`)
  }
  const report = JSON.parse(result.stdout)
  if (report.error || !report.vulnerabilities) {
    throw new Error(`npm audit returned an invalid report: ${report.error?.summary ?? 'missing vulnerabilities'}`)
  }
  return report.vulnerabilities
}

const production = audit(['--omit=dev'])
if (Object.keys(production).length > 0) {
  throw new Error(`Runtime dependencies have advisories: ${Object.keys(production).join(', ')}`)
}

const allowedPaths = new Map([
  ['eslint-config-next', '@next/eslint-plugin-next'],
  ['@next/eslint-plugin-next', 'fast-glob'],
  ['fast-glob', 'micromatch'],
  ['micromatch', 'braces'],
])
const advisory = 'https://github.com/advisories/GHSA-vfj7-8cjw-p6xm'
const full = audit()
for (const [name, issue] of Object.entries(full)) {
  if (name === 'braces') {
    if (issue.via.length !== 1 || issue.via[0]?.url !== advisory) {
      throw new Error('braces has a new or changed advisory')
    }
  } else if (allowedPaths.has(name)) {
    if (issue.via.length !== 1 || issue.via[0] !== allowedPaths.get(name)) {
      throw new Error(`${name} has a new or changed advisory path`)
    }
  } else {
    throw new Error(`New frontend dependency advisory: ${name}`)
  }
}
console.log('Frontend audit passed: runtime dependencies clean; known ESLint-only braces advisory reviewed')
