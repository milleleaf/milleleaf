import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/status')({
  head: () => ({ meta: [{ title: 'Status | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/status' }] }),
  component: () => <App path='/status' />,
})
