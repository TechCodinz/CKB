export default async function handler(req, res) {
    if (req.method !== 'POST') {
        return res.status(405).json({ message: 'Method Not Allowed' });
    }

    const signature = req.headers['x-cc-webhook-signature'];
    const webhookSecret = process.env.COINBASE_COMMERCE_WEBHOOK_SECRET;

    // Note: In a production environment, you should verify the signature here
    // using the webhookSecret to ensure the request came from Coinbase.

    try {
        if (req.body.event && req.body.event.type === 'charge:confirmed') {
            const { customer_email, plan } = req.body.event.data.metadata;

            console.log(`Charge confirmed for ${customer_email}, plan: ${plan}`);

            // Here you would typically:
            // 1. Grant access in your database (e.g., Supabase)
            // await grantAccess(customer_email, plan);

            // 2. Send welcome email via Resend
            // await sendWelcomeEmail(customer_email);
        }

        res.json({ received: true });
    } catch (error) {
        console.error('Webhook processing error:', error);
        res.status(500).json({ error: 'Webhook processing failed' });
    }
}
