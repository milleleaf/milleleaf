import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/data/datasets')({
  head: () => ({ meta: [{ title: 'Datasets | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/data/datasets' }] }),
  component: () => <App path='/data/datasets' />,
})
