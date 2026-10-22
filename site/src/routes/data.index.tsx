import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/data/')({ component: () => <App path="/data" /> })
