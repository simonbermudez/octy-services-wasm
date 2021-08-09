from config import Config


######################################
# Notification payload content
######################################
'''
NOTE: If Dict format is required for webhook, pull content from here and build 
dict object after formatting required values.
Example webhook payload : 

{
"subject" : AUTH_SECURITY_WARNING_SUBJECT, 
"body" : AUTH_SECURITY_WARNING_BODY.format(...), 
"date_time" : "Tue, 28 Jun 2020 10:09:15 GMT"
}

'''


#ACCOUNT CREATION

ACCOUNT_SUBJECT='Your Octy account has been created.'
ACCOUNT_BODY="Dear {first_name},\n\n\
If we haven't already been introduced, my name is Ben.\n\
As the founder, I would like personally thank you for making an account with Octy. I am very happy you made the step to make the most of your data using Octy's toolchains.\n\n\
This email contains sensitive information, specifically your API keys.\n\
Your API keys should be kept secure and only provided to trusted third parties or individuals as they grant access to all resources associated with your account.\n\n\
Step one: Safely store your API keys\n\
Step two: Go to our Docs [{link}] to get started with integrating Octy with your systems.\n\
Step three: Delete this email to ensure your API Keys exist in a secure place only.\n\n\
YOUR API KEYS:\n\
PUBLIC KEY: {pk}\n\
SECRET KEY: {sk}\n\n\
If you have any questions, queries or issues, you can open a support ticket at support@octy.ai\n\n\
Ben"


#AUTH SECURITY WARNING

AUTH_SECURITY_WARNING_SUBJECT='Octy Account alert [IMPORTANT]'
AUTH_SECURITY_WARNING_BODY='We have noticed unusual activity associated with your account.\n\
Someone has attempted to authenticate against your accounts public key more than '+str(Config['FAILED_AUTH_ATTEMPT_LIMIT'])+' times in the past 30 minutes.\n\
If this was you or someone from your team you do not need to do anything as this is simply a security warning, however,\n\
If this action did not come from any authorized personal, please contact us immediately: '+Config['SUPPORT_EMAIL']