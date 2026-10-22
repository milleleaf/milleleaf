import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/standards')({
  head: () => ({ meta: [{ title: 'Standards | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/standards' }] }),
  component: () => <App path='/standards' />,
})
