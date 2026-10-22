import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/docs/getting-started')({
  head: () => ({ meta: [{ title: 'Getting Started | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/docs/getting-started' }] }),
  component: () => <App path='/docs/getting-started' />,
})
