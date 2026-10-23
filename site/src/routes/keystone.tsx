import { createFileRoute } from '@tanstack/react-router'
import { App } from '../app'
export const Route = createFileRoute('/keystone')({
  head: () => ({ meta: [{ title: 'Keystone | Milleleaf' }], links: [{ rel: 'canonical', href: 'https://milleleaf.com/keystone' }] }),
  component: () => <App path='/keystone' />,
})
