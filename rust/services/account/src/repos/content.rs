//! Port of `data/repositories/content/notification_content.py`.

pub const ACCOUNT_SUBJECT: &str = "Your Octy account has been created.";

pub fn account_body(first_name: &str, link: &str, pk: &str, sk: &str) -> String {
    format!(
        "Hello {first_name},\n\n\
Thank you for starting your Octy journey. We are delighted that you chose Octy.\n\
Thank you for making an account with us. Our team is dedicated to helping you make the most of your data using Octy's toolchains.\n\n\
This email contains sensitive information: your API keys.\n\
You'll need to keep these secure and only provide them to trusted third parties or individuals as they grant access to all resources associated with your account.\n\n\
Step one: Safely store your API keys\n\
Step two: Go to our Docs [{link}] to get started with integrating Octy with your systems.\n\
Step three: Delete this email so that no one gets their hands on your API keys but you!\n\n\
==================================================== \n\
YOUR API KEYS:\n\
PUBLIC KEY: {pk}\n\
SECRET KEY: {sk}\n\n\
==================================================== \n\n\
You can contact us at support@octy.ai if you have any questions.\n\
Your Octy Support Team"
    )
}

pub const AUTH_SECURITY_WARNING_SUBJECT: &str = "Octy Account alert [IMPORTANT]";

pub fn auth_security_warning_body(failed_auth_attempt_limit: i64, support_email: &str) -> String {
    format!(
        "We have noticed unusual activity associated with your account.\n\
Someone has attempted to authenticate against your accounts public key more than {failed_auth_attempt_limit} times in the past 30 minutes.\n\
If this was you or someone from your team you do not need to do anything as this is simply a security warning, however,\n\
If this action did not come from any authorized personal, please contact us immediately: {support_email}"
    )
}
