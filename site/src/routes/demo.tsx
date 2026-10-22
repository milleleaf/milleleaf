import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/demo')({
  head: () => ({ meta: [{ title: 'Demo | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/demo' }] }),
  component: () => <App path='/demo' />,
})
