export default async function handler(req, res) {
    if (req.method !== 'POST') {
        return res.status(405).json({ message: 'Method Not Allowed' });
    }

    try {
        const response = await fetch('https://api.commerce.coinbase.com/charges', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                'X-CC-Api-Key': process.env.COINBASE_COMMERCE_API_KEY,
            },
            body: JSON.stringify({
                name: 'CKB Pro',
                description: 'Monthly subscription',
                pricing_type: 'fixed_price',
                local_price: {
                    amount: '29.00',
                    currency: 'USD'
                },
                metadata: {
                    customer_email: req.body.email,
                    plan: 'pro'
                },
                redirect_url: 'https://ckb.vercel.app/success',
                cancel_url: 'https://ckb.vercel.app/pricing'
            })
        });

        const charge = await response.json();
        res.json({ hosted_url: charge.data.hosted_url });
    } catch (error) {
        console.error('Error creating charge:', error);
        res.status(500).json({ error: 'Internal Server Error' });
    }
}
