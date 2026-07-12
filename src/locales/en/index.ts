import common from './common.json'
import onboarding from './onboarding.json'
import settings from './settings.json'
import network from './network.json'
import credentials from './credentials.json'
import courses from './courses.json'
import learn from './learn.json'
import skills from './skills.json'
import opinions from './opinions.json'
import governance from './governance.json'
import tutoring from './tutoring.json'
import classrooms from './classrooms.json'
import instructor from './instructor.json'
import guardian from './guardian.json'
import plugins from './plugins.json'
import profile from './profile.json'
import goals from './goals.json'
import sentinel from './sentinel.json'
import nav from './nav.json'
import dashboard from './dashboard.json'
import omni from './omni.json'

const messages = { common, onboarding, settings, network, credentials, courses, learn, skills, opinions, governance, tutoring, classrooms, instructor, guardian, plugins, profile, goals, sentinel, nav, dashboard, omni }

// The English catalog is the canonical shape all other locales conform to.
export type LocaleMessages = typeof messages

export default messages
