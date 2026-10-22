import { defineConfig } from '@rsbuild/core'
import { pluginReact } from '@rsbuild/plugin-react'
import { tanstackStart } from '@tanstack/react-start/plugin/rsbuild'

export default defineConfig({
  plugins: [pluginReact(), tanstackStart({
    prerender: { enabled: true, crawlLinks: true, concurrency: 1, failOnError: true, filter: ({ path }) => !path.includes('#') && !/\.[a-z0-9]+$/i.test(path) },
  })],
})
