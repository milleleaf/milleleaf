import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/flow')({
  head: () => ({ meta: [{ title: 'Make it once: demo | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/flow' }] }),
  component: () => <App path='/flow' />,
})
