// npm ci && npm run build regenerates the offline browser module from Google's release.
import { build } from 'esbuild'
import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dir = await mkdtemp(join(tmpdir(), 'workout-assist-'))
const files = {
  'model.json':
    'c65a3447162efa0c42af29498a4b151f4415b36ff3ae6fae2b28a32e840dcbfb',
  'group1-shard1of2.bin':
    'b42c3232bf13b0efc691d3f0693dd3fc74404f709b1deee0a271828af6dbbea2',
  'group1-shard2of2.bin':
    '8253ab965aa3122c08f331777ca395ce9ca19bb9e7cb278b99de1c46dbdeb6dc',
}
try {
  const url =
    'https://www.kaggle.com/api/v1/models/google/movenet/tfJs/singlepose-lightning/4/download'
  execFileSync('curl', [
    '--fail',
    '--location',
    '--silent',
    '--show-error',
    url,
    '-o',
    join(dir, 'model.tgz'),
  ])
  execFileSync('tar', [
    '-xzf',
    join(dir, 'model.tgz'),
    '-C',
    dir,
    ...Object.keys(files),
  ])
  for (const [file, expected] of Object.entries(files)) {
    const bytes = await readFile(join(dir, file))
    if (createHash('sha256').update(bytes).digest('hex') !== expected)
      throw Error('Unexpected ' + file)
  }
  const model = JSON.parse(await readFile(join(dir, 'model.json'), 'utf8'))
  const weights = Buffer.concat(
    await Promise.all(
      model.weightsManifest[0].paths.map((p) => readFile(join(dir, p))),
    ),
  )
  await writeFile(join(dir, 'weights.bin'), weights)
  await build({
    entryPoints: ['inference.js'],
    outfile: '../ui/vendor/movenet.js',
    alias: {
      'pose-graph': join(dir, 'model.json'),
      'pose-weights': join(dir, 'weights.bin'),
    },
    loader: { '.bin': 'binary' },
    bundle: true,
    format: 'esm',
    minify: true,
    legalComments: 'eof',
    target: 'es2020',
  })
} finally {
  await rm(dir, { recursive: true, force: true })
}
