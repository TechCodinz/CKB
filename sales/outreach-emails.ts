export const salesTemplates = {
    // Initial outreach to engineering leaders
    initialOutreach: (contact: { name: string; company: string; role: string }) => `
Subject: ${contact.company} using AI coding tools? Let's talk architecture

Hi ${contact.name},

I noticed ${contact.company} is building with modern tools, and like many teams, you're probably using AI assistants (Cursor, Copilot, Claude) to accelerate development.

Here's the problem Taylor Otwell recently highlighted:

> "When you're using an LLM, you keep pushing right through in a way that feels like progress, but the underlying foundation is actually bad."

That's exactly what we built CKB to solve.

CKB gives AI assistants architectural awareness:
- Real-time drift detection when AI code violates your architecture
- MCP server so Cursor/Claude query your architecture before generating code
- Impact analysis showing exactly what breaks before any change

Teams using CKB have cut technical debt from AI-generated code by 70% and maintained velocity without sacrificing quality.

Would you be open to a 15-min demo this week?

Best,
[Your name]
Founder, CKB
  `,

    // Follow-up after demo
    followUp: (contact: { name: string; company: string }) => `
Subject: Next steps with CKB for ${contact.company}

Hi ${contact.name},

Great talking with you earlier! Based on our conversation, here's how CKB would specifically help ${contact.company}:

Key benefits for your team:
- 🔍 **Real-time detection** of architectural violations in AI-generated code
- 🤖 **MCP integration** so Cursor/Claude understand your architecture
- 📊 **Team dashboard** with violation trends and health metrics
- 🔒 **SSO/SAML** (Enterprise plan)

Next steps:
1. [ ] 14-day free trial: [link]
2. [ ] I'll set up a technical deep-dive with your team
3. [ ] We'll review findings after first week

Questions? I'm here to help.

Best,
[Your name]
  `,

    // Case study sharing
    caseStudy: (contact: { name: string; company: string }) => `
Subject: How [Similar Company] cut AI code debt by 70% with CKB

Hi ${contact.name},

Thought you'd find this interesting: [Similar Company] was facing the exact challenge you mentioned—AI-generated code accelerating development but creating hidden architectural debt.

After implementing CKB:

Results after 30 days:
- ✅ 73% reduction in architectural violations
- ✅ 40% faster onboarding for new devs
- ✅ Zero production incidents from AI-generated code

Full case study: [link]

Want to see how ${contact.company} compares? I can run a free assessment of your codebase.

Best,
[Your name]
  `,

    // Abandoned trial
    abandonedTrial: (contact: { name: string; company: string }) => `
Subject: Still thinking about CKB for ${contact.company}?

Hi ${contact.name},

Noticed you haven't had a chance to fully explore CKB yet. Totally understand—everyone's busy!

Quick wins you might have missed:
- 🎯 **VS Code extension** gives real-time feedback as you code
- 🤖 **MCP server** connects Cursor/Claude to your architecture in 5 min
- 📈 **Dashboard** shows your team's architectural health at a glance

Still have questions? I'd love to hop on a quick call.

Best,
[Your name]
  `,

    // Enterprise follow-up
    enterpriseFollowUp: (contact: { name: string; company: string; requirements: any }) => `
Subject: CKB Enterprise for ${contact.company} - Proposal attached

Hi ${contact.name},

Following up on our discussion about ${contact.company}'s requirements:

Your needs:
- On-premise deployment ✅
- SSO/SAML integration ✅
- 99.9% SLA ✅
- Dedicated support engineer ✅
- Custom rule development ✅

I've attached a detailed proposal with pricing tailored to your scale.

Key points:
- Volume discount: [X]% off list price
- Annual contract: [Y]% discount
- Implementation timeline: 2-3 weeks

Happy to walk through it whenever you're ready.

Best,
[Your name]
  `,
};
