import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/data/round-trip')({
  head: () => ({ meta: [{ title: 'Round Trip | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/data/round-trip' }] }),
  component: () => <App path='/data/round-trip' />,
})
